//! CUDA と wgpu が共有する D3D12 のバッファの輪。cuvid のコマを 1 回コピーして wgpu に渡す。
//!
//! 各枠のフェンスは CUDA と wgpu が交互に進める。値は 1, 2, 3, ... と増え、
//! 奇数が「CUDA がコピーを終えた」、偶数が「wgpu が読み終えた」。CUDA は次のコピーの前に
//! 前の偶数まで待つので、wgpu が読んでいる最中の枠を上書きしない。

use std::sync::Arc;

use mm_decode_hw::device::{queue_signal, queue_wait};
use mm_types::{MmError, Result};
use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12Device, ID3D12DeviceChild, ID3D12Fence, ID3D12Resource, D3D12_FENCE_FLAG_SHARED,
    D3D12_HEAP_FLAG_SHARED, D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_DESC,
    D3D12_RESOURCE_DIMENSION_BUFFER, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COMMON,
    D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

use crate::cuda::{
    CuContext, CuDevicePtr, CuExternalMemory, CuExternalSemaphore, CuStream, CudaApi, Memcpy2D,
    MEMORYTYPE_DEVICE,
};
use crate::hooks::{self, Stage};

/// `GENERIC_ALL`。共有ハンドルのアクセス権。
const GENERIC_ALL: u32 = 0x1000_0000;

/// 共有バッファの中の配置。Y 面の後に UV 面が続き、行の間に余白は無い。
/// 色差の面は 4:2:0 なので、幅と高さが奇数なら切り上げた (w+1)/2 列 x (h+1)/2 行になる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferLayout {
    pub height: u32,
    /// Y 面の 1 行のバイト数。
    pub pitch: u32,
    /// UV 面の 1 行のバイト数（U と V が交互に並ぶ (w+1)/2 組）。
    pub uv_pitch: u32,
    /// UV 面の行数。
    pub uv_rows: u32,
    /// UV 面の先頭のバイト位置。
    pub uv_offset: u32,
    /// バッファ全体のバイト数（4 の倍数）。
    pub size: u64,
}

impl BufferLayout {
    /// 輝度の大きさと、1 成分のバイト数（8 bit は 1、10 bit は 2）から。
    pub fn new(width: u32, height: u32, bytes_per_sample: u32) -> Self {
        let pitch = width * bytes_per_sample;
        let uv_pitch = width.div_ceil(2) * 2 * bytes_per_sample;
        let uv_rows = height.div_ceil(2);
        let uv_offset = pitch * height;
        let size = u64::from(uv_offset) + u64::from(uv_pitch) * u64::from(uv_rows);
        Self {
            height,
            pitch,
            uv_pitch,
            uv_rows,
            uv_offset,
            size: size.next_multiple_of(4),
        }
    }
}

/// 取り込んだ CUDA の資源（外部メモリとそのポインタ、外部セマフォ）。捨てるときに文脈を現在にして手放すので、
/// 輪を作る途中で失敗しても、枠を捨てたときも漏れない。GPU の仕事が終わってから捨てる。
struct Imports {
    cuda: Arc<CudaApi>,
    ctx: CuContext,
    memory: Option<(CuExternalMemory, CuDevicePtr)>,
    semaphore: Option<CuExternalSemaphore>,
}

impl Imports {
    fn new(cuda: Arc<CudaApi>, ctx: CuContext) -> Self {
        Self {
            cuda,
            ctx,
            memory: None,
            semaphore: None,
        }
    }

    /// 共有バッファのハンドルを取り込む。
    ///
    /// # Safety
    /// 文脈が現在で、`handle` は `size` バイトの共有バッファの NT ハンドル。
    unsafe fn import_memory(&mut self, handle: HANDLE, size: u64) -> Result<()> {
        let (mem, ptr) = unsafe { self.cuda.import_buffer(handle.0, size)? };
        self.memory = Some((mem, ptr));
        hooks::imported();
        Ok(())
    }

    /// 共有フェンスのハンドルを取り込む。
    ///
    /// # Safety
    /// 文脈が現在で、`handle` は共有フェンスの NT ハンドル。
    unsafe fn import_semaphore(&mut self, handle: HANDLE) -> Result<()> {
        self.semaphore = Some(unsafe { self.cuda.import_fence(handle.0)? });
        hooks::imported();
        Ok(())
    }

    fn ptr(&self) -> CuDevicePtr {
        self.memory.map_or(0, |(_, p)| p)
    }

    fn semaphore(&self) -> CuExternalSemaphore {
        self.semaphore.unwrap_or(std::ptr::null_mut())
    }
}

impl Drop for Imports {
    fn drop(&mut self) {
        if self.memory.is_none() && self.semaphore.is_none() {
            return;
        }
        // SAFETY: 文脈は復号器のもの。呼び出し側が GPU の仕事の完了を保証する。
        let Ok(_ctx) = (unsafe { self.cuda.scoped(self.ctx) }) else {
            return;
        };
        if let Some(sem) = self.semaphore.take() {
            // SAFETY: この文脈で取り込んだセマフォ。
            unsafe { self.cuda.release_semaphore(sem) };
            hooks::released();
        }
        if let Some((mem, ptr)) = self.memory.take() {
            // SAFETY: この文脈で取り込んだ外部メモリとポインタ。
            unsafe { self.cuda.release_memory(mem, ptr) };
            hooks::released();
        }
    }
}

/// 輪の 1 枠。
pub struct Slot {
    pub buffer: wgpu::Buffer,
    fence: ID3D12Fence,
    /// これまでにコピーを積めた回数。
    copies: u64,
    imports: Imports,
}

impl Slot {
    /// 直近のコピーの完了を表すフェンスの値。
    pub fn copy_done_value(&self) -> u64 {
        2 * self.copies - 1
    }

    /// 直近のコピーを wgpu が読み終えたことを表すフェンスの値。
    pub fn read_done_value(&self) -> u64 {
        2 * self.copies
    }

    /// 共有フェンス。
    pub fn fence(&self) -> &ID3D12Fence {
        &self.fence
    }
}

/// 1 面のコピー元とコピー先。
struct PlaneCopy {
    src: CuDevicePtr,
    src_pitch: usize,
    dst_offset: u64,
    dst_pitch: usize,
    rows: usize,
}

/// 共有バッファの輪。
pub struct SharedRing {
    device: wgpu::Device,
    queue: wgpu::Queue,
    cuda: Arc<CudaApi>,
    ctx: CuContext,
    stream: CuStream,
    slots: Vec<Slot>,
    next: usize,
    layout: BufferLayout,
    /// 検査で待ちを外した対照を作るための印。本番では常に `true`。
    wait_for_reader: bool,
}

impl SharedRing {
    /// `layout` の配置の共有バッファを `count` 枠つくる。
    ///
    /// # Safety
    /// `ctx` と `stream` は、復号器が使う有効な CUDA の文脈とストリーム。
    pub unsafe fn new(
        (device, queue): (wgpu::Device, wgpu::Queue),
        cuda: Arc<CudaApi>,
        (ctx, stream): (CuContext, CuStream),
        layout: BufferLayout,
        count: usize,
    ) -> Result<Self> {
        let slots = {
            // SAFETY: 文脈を現在にしている間だけ CUDA を呼ぶ。外れるのはこのブロックを抜けるとき。
            let _ctx = unsafe { cuda.scoped(ctx)? };
            (0..count)
                .map(|_| make_slot(&device, &cuda, ctx, layout.size))
                .collect::<Result<Vec<Slot>>>()?
        };
        Ok(Self {
            device,
            queue,
            cuda,
            ctx,
            stream,
            slots,
            next: 0,
            layout,
            wait_for_reader: true,
        })
    }

    /// 共有バッファの配置。
    pub fn layout(&self) -> BufferLayout {
        self.layout
    }

    /// CUDA が wgpu の読み終えを待つかを切り替える（待ちが効いていることを確かめる対照の検査用）。
    #[cfg(feature = "test-hooks")]
    pub fn set_wait_for_reader(&mut self, on: bool) {
        self.wait_for_reader = on;
    }

    /// 枠の数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 枠の中身。
    pub fn slot(&self, index: usize) -> &Slot {
        &self.slots[index]
    }

    /// cuvid のコマ（CUDA の NV12 か P010）を次の枠にコピーする。CPU は待たない。
    /// その枠を wgpu が前に読んだ分の完了を CUDA のストリームの中で待ってからコピーし、
    /// 終わったらフェンスを進める。戻り値は枠の番号。
    ///
    /// 途中で失敗したら枠の回数を進めない。完了のシグナルを積めなかったコピーを数えると、
    /// 次にその枠を使うときの待ちが永遠に満たされず、CUDA のストリームが止まるため。
    pub fn copy_frame(&mut self, f: &ffmpeg_next::frame::Video) -> Result<usize> {
        let index = self.next;
        let planes = self.planes_of(f);
        let slot = &self.slots[index];
        let (previous_read, copy_done) = (2 * slot.copies, 2 * slot.copies + 1);
        {
            // SAFETY: 文脈を現在にしている間にだけ CUDA を呼ぶ。外れるのはこのブロックを抜けるとき。
            let _ctx = unsafe { self.cuda.scoped(self.ctx)? };
            if previous_read > 0 && self.wait_for_reader {
                // SAFETY: 取り込み済みのセマフォ。
                unsafe {
                    self.cuda
                        .wait(slot.imports.semaphore(), previous_read, self.stream)?
                };
            }
            self.enqueue_copies(slot, &planes)?;
            hooks::fault(Stage::Signal)?;
            // SAFETY: 取り込み済みのセマフォ。
            unsafe {
                self.cuda
                    .signal(slot.imports.semaphore(), copy_done, self.stream)?
            };
        }
        self.slots[index].copies += 1;
        self.next = (index + 1) % self.slots.len();
        Ok(index)
    }

    /// cuvid のコマの 2 面（Y と UV）のコピー元と、共有バッファの中のコピー先。
    fn planes_of(&self, f: &ffmpeg_next::frame::Video) -> [PlaneCopy; 2] {
        let l = self.layout;
        // SAFETY: cuvid のコマは CUDA のデバイスポインタを data[0] と data[1] に持つ。
        let (raw, y_pitch, uv_pitch) = unsafe {
            let raw = f.as_ptr();
            (raw, (*raw).linesize[0], (*raw).linesize[1])
        };
        [
            PlaneCopy {
                src: unsafe { (*raw).data[0] } as CuDevicePtr,
                src_pitch: y_pitch as usize,
                dst_offset: 0,
                dst_pitch: l.pitch as usize,
                rows: l.height as usize,
            },
            PlaneCopy {
                src: unsafe { (*raw).data[1] } as CuDevicePtr,
                src_pitch: uv_pitch as usize,
                dst_offset: u64::from(l.uv_offset),
                dst_pitch: l.uv_pitch as usize,
                rows: l.uv_rows as usize,
            },
        ]
    }

    /// 2 面のコピーをストリームに積む。文脈が現在のときに呼ぶ。
    fn enqueue_copies(&self, slot: &Slot, planes: &[PlaneCopy; 2]) -> Result<()> {
        hooks::fault(Stage::Memcpy)?;
        for p in planes {
            let copy = Memcpy2D {
                src_x_in_bytes: 0,
                src_y: 0,
                src_memory_type: MEMORYTYPE_DEVICE,
                src_host: std::ptr::null(),
                src_device: p.src,
                src_array: std::ptr::null_mut(),
                src_pitch: p.src_pitch,
                dst_x_in_bytes: 0,
                dst_y: 0,
                dst_memory_type: MEMORYTYPE_DEVICE,
                dst_host: std::ptr::null_mut(),
                dst_device: slot.imports.ptr() + p.dst_offset,
                dst_array: std::ptr::null_mut(),
                dst_pitch: p.dst_pitch,
                width_in_bytes: p.dst_pitch,
                height: p.rows,
            };
            // SAFETY: 文脈が現在で、両端は有効なデバイスメモリ。
            unsafe { self.cuda.memcpy2d_async(&copy, self.stream)? };
        }
        Ok(())
    }

    /// 次の `Queue::submit` の頭で、枠のコピーの完了までキューを待たせる（CPU は待たない）。
    pub fn wait_copy_on(&self, index: usize) -> Result<()> {
        let slot = &self.slots[index];
        queue_wait(&self.queue, &slot.fence, slot.copy_done_value())
    }

    /// 枠を読む提出を終えたら、wgpu のキューに「読み終えた」値までフェンスを進めさせる。
    /// 提出のあとに呼ぶ。CUDA は次のコピーの前にこれを待つ。
    ///
    /// キューに積めなかったときは CPU からフェンスを進める。進めないと次のコピーの待ちが満たされず
    /// CUDA のストリームが止まる。この枠の絵は呼び出し側が捨てる（エラーを返す）ので、早く進めてよい。
    pub fn signal_read_done(&self, index: usize) -> Result<()> {
        let slot = &self.slots[index];
        let value = slot.read_done_value();
        let queued = queue_signal(&self.queue, &slot.fence, value);
        if queued.is_err() {
            // SAFETY: 有効なフェンス。CPU からの Signal。
            let _ = unsafe { slot.fence.Signal(value) };
        }
        queued
    }

    /// wgpu のデバイス。
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
}

impl Drop for SharedRing {
    fn drop(&mut self) {
        // wgpu 側の読み取りが残っていれば終わるまで待ち、CUDA のコピーも終えてから取り込みを外す
        // （外すのは枠の `Imports` の Drop）。
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        // SAFETY: 文脈とストリームは復号器のもの。
        if let Ok(_ctx) = unsafe { self.cuda.scoped(self.ctx) } {
            let _ = unsafe { self.cuda.sync(self.stream) };
        }
        self.slots.clear();
    }
}

fn d3d_err(what: &str, e: windows::core::Error) -> MmError {
    MmError::Internal(format!("{what}: {e}"))
}

/// 共有ハンドルを作って `import` に渡し、渡し終えたら閉じる（CUDA が取り込み時に自前の参照を持つ）。
///
/// # Safety
/// `object` は `dev` の有効な共有可能なオブジェクト。
unsafe fn with_shared_handle(
    dev: &ID3D12Device,
    object: &impl Interface,
    what: &str,
    import: impl FnOnce(HANDLE) -> Result<()>,
) -> Result<()> {
    let object: ID3D12DeviceChild = object.cast().map_err(|e| d3d_err(what, e))?;
    let handle: HANDLE =
        unsafe { dev.CreateSharedHandle(&object, None, GENERIC_ALL, PCWSTR::null()) }
            .map_err(|e| d3d_err(what, e))?;
    let imported = import(handle);
    let _ = unsafe { CloseHandle(handle) };
    imported
}

fn shared_buffer_desc(size: u64) -> D3D12_RESOURCE_DESC {
    D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Alignment: 0,
        Width: size,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_UNKNOWN,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        Flags: D3D12_RESOURCE_FLAG_NONE,
    }
}

/// wgpu と同じデバイスに共有バッファとフェンスを作り、CUDA に取り込む。文脈は現在にしておく。
fn make_slot(
    device: &wgpu::Device,
    cuda: &Arc<CudaApi>,
    ctx: CuContext,
    size: u64,
) -> Result<Slot> {
    hooks::fault(Stage::SlotImport)?;
    // SAFETY: as_hal の参照はこの関数の間だけ使う。資源は wgpu と同じデバイスに作る。
    unsafe {
        let hal = device
            .as_hal::<wgpu::hal::api::Dx12>()
            .ok_or_else(|| MmError::Unsupported("wgpu が D3D12 ではない".into()))?;
        let dev = hal.raw_device();
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };
        let mut resource: Option<ID3D12Resource> = None;
        dev.CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_SHARED,
            &shared_buffer_desc(size),
            D3D12_RESOURCE_STATE_COMMON,
            None,
            &mut resource,
        )
        .map_err(|e| d3d_err("共有バッファの作成", e))?;
        let resource = resource.ok_or_else(|| MmError::Internal("共有バッファが空".into()))?;
        let fence: ID3D12Fence = dev
            .CreateFence(0, D3D12_FENCE_FLAG_SHARED)
            .map_err(|e| d3d_err("共有フェンス", e))?;

        let mut imports = Imports::new(cuda.clone(), ctx);
        with_shared_handle(dev, &resource, "共有ハンドル", |h| {
            imports.import_memory(h, size)
        })?;
        with_shared_handle(dev, &fence, "フェンスの共有ハンドル", |h| {
            imports.import_semaphore(h)
        })?;

        let buffer = device.create_buffer_from_hal::<wgpu::hal::api::Dx12>(
            wgpu::hal::dx12::Device::buffer_from_raw(resource, size),
            &wgpu::BufferDescriptor {
                label: Some("cuvid shared yuv420"),
                size,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            },
        );
        Ok(Slot {
            buffer,
            fence,
            copies: 0,
            imports,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 偶数の大きさは行の間に余白がなく_色差は半分になる() {
        let l = BufferLayout::new(1920, 1080, 1);
        assert_eq!((l.pitch, l.uv_pitch, l.uv_rows), (1920, 1920, 540));
        assert_eq!(l.uv_offset, 1920 * 1080);
        assert_eq!(l.size, 1920 * 1080 * 3 / 2);
    }

    #[test]
    fn 奇数の幅と高さは色差の最後の列と行を切り捨てない() {
        let l = BufferLayout::new(7, 5, 1);
        // 色差は 4 列 x 3 行。U と V が交互なので 1 行 8 バイト。
        assert_eq!((l.pitch, l.uv_pitch, l.uv_rows), (7, 8, 3));
        assert_eq!(l.uv_offset, 35);
        assert_eq!(l.size, (35u64 + 8 * 3).next_multiple_of(4));
    }

    #[test]
    fn 十ビットは一成分が二バイトで色差の行も倍になる() {
        let l = BufferLayout::new(7, 5, 2);
        assert_eq!((l.pitch, l.uv_pitch, l.uv_rows), (14, 16, 3));
    }
}
