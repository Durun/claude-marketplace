//! CUDA と wgpu が共有する D3D12 のバッファの輪。cuvid のコマを 1 回コピーして wgpu に渡す。
//!
//! 各枠のフェンスは CUDA と wgpu が交互に進める。値は 1, 2, 3, ... と増え、
//! 奇数が「CUDA がコピーを終えた」、偶数が「wgpu が読み終えた」。CUDA は次のコピーの前に
//! 前の偶数まで待つので、wgpu が読んでいる最中の枠を上書きしない。

use std::sync::Arc;

use mm_types::{MmError, Result};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12Fence, ID3D12Resource, D3D12_FENCE_FLAG_SHARED, D3D12_HEAP_FLAG_SHARED,
    D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_DESC,
    D3D12_RESOURCE_DIMENSION_BUFFER, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COMMON,
    D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

use crate::cuda::{
    CuContext, CuDevicePtr, CuExternalMemory, CuExternalSemaphore, CuStream, CudaApi, Memcpy2D,
    MEMORYTYPE_DEVICE,
};

/// `GENERIC_ALL`。共有ハンドルのアクセス権。
const GENERIC_ALL: u32 = 0x1000_0000;

/// 輪の 1 枠。
pub struct Slot {
    pub buffer: wgpu::Buffer,
    cu_ptr: CuDevicePtr,
    fence: ID3D12Fence,
    cu_sem: CuExternalSemaphore,
    /// これまでにコピーした回数。
    copies: u64,
    cu_mem: CuExternalMemory,
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

/// 共有バッファの輪。
pub struct SharedRing {
    device: wgpu::Device,
    queue: wgpu::Queue,
    cuda: Arc<CudaApi>,
    ctx: CuContext,
    stream: CuStream,
    slots: Vec<Slot>,
    next: usize,
    width: u32,
    height: u32,
    /// 1 成分のバイト数（8 bit は 1、10 bit は 2）。
    bytes_per_sample: u32,
    /// 検査で待ちを外した対照を作るための印。本番では常に `true`。
    wait_for_reader: bool,
}

impl SharedRing {
    /// Y 面の後に UV 面が続く 1 本のバッファ（行の間に余白なし）を `count` 枠つくる。
    ///
    /// # Safety
    /// `ctx` と `stream` は、復号器が使う有効な CUDA の文脈とストリーム。
    pub unsafe fn new(
        (device, queue): (wgpu::Device, wgpu::Queue),
        cuda: Arc<CudaApi>,
        (ctx, stream): (CuContext, CuStream),
        (width, height): (u32, u32),
        bytes_per_sample: u32,
        count: usize,
    ) -> Result<Self> {
        let size = u64::from(width) * u64::from(height) * 3 / 2 * u64::from(bytes_per_sample);
        let size = size.next_multiple_of(4);
        // SAFETY: 文脈を現在にしている間だけ CUDA を呼ぶ。
        unsafe { cuda.push(ctx)? };
        let slots: Result<Vec<Slot>> = (0..count)
            .map(|_| make_slot(&device, &cuda, size))
            .collect();
        // SAFETY: 上の push と対。
        unsafe { cuda.pop()? };
        Ok(Self {
            device,
            queue,
            cuda,
            ctx,
            stream,
            slots: slots?,
            next: 0,
            width,
            height,
            bytes_per_sample,
            wait_for_reader: true,
        })
    }

    /// 1 行のバイト数。
    pub fn pitch(&self) -> u32 {
        self.width * self.bytes_per_sample
    }

    /// UV 面の先頭のバイト位置。
    pub fn uv_offset(&self) -> u32 {
        self.pitch() * self.height
    }

    /// CUDA が wgpu の読み終えを待つかを切り替える（待ちが効いていることを確かめる対照の検査用）。
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
    pub fn copy_frame(&mut self, f: &ffmpeg_next::frame::Video) -> Result<usize> {
        let index = self.next;
        self.next = (self.next + 1) % self.slots.len();
        let pitch = self.pitch() as usize;
        let uv_offset = u64::from(self.uv_offset());
        let slot = &mut self.slots[index];
        let previous_read = 2 * slot.copies;
        slot.copies += 1;
        let copy_done = slot.copy_done_value();
        // SAFETY: cuvid のコマは CUDA のデバイスポインタを data[0] と data[1] に持つ。
        // 文脈を現在にしている間にだけ CUDA を呼ぶ。
        unsafe {
            let raw = f.as_ptr();
            let planes = [
                (
                    (*raw).data[0] as CuDevicePtr,
                    (*raw).linesize[0],
                    self.height,
                    0u64,
                ),
                (
                    (*raw).data[1] as CuDevicePtr,
                    (*raw).linesize[1],
                    self.height / 2,
                    uv_offset,
                ),
            ];
            self.cuda.push(self.ctx)?;
            if previous_read > 0 && self.wait_for_reader {
                self.cuda.wait(slot.cu_sem, previous_read, self.stream)?;
            }
            for (src, src_pitch, rows, dst_offset) in planes {
                let copy = Memcpy2D {
                    src_x_in_bytes: 0,
                    src_y: 0,
                    src_memory_type: MEMORYTYPE_DEVICE,
                    src_host: std::ptr::null(),
                    src_device: src,
                    src_array: std::ptr::null_mut(),
                    src_pitch: src_pitch as usize,
                    dst_x_in_bytes: 0,
                    dst_y: 0,
                    dst_memory_type: MEMORYTYPE_DEVICE,
                    dst_host: std::ptr::null_mut(),
                    dst_device: slot.cu_ptr + dst_offset,
                    dst_array: std::ptr::null_mut(),
                    dst_pitch: pitch,
                    width_in_bytes: pitch,
                    height: rows as usize,
                };
                self.cuda.memcpy2d_async(&copy, self.stream)?;
            }
            self.cuda.signal(slot.cu_sem, copy_done, self.stream)?;
            self.cuda.pop()?;
        }
        Ok(index)
    }

    /// 次の `Queue::submit` の頭で、枠のコピーの完了までキューを待たせる（CPU は待たない）。
    pub fn wait_copy_on(&self, index: usize) -> Result<()> {
        let slot = &self.slots[index];
        // SAFETY: as_hal の参照はこの呼び出しの間だけ使う。
        let queue = unsafe { self.queue.as_hal::<wgpu::hal::api::Dx12>() }
            .ok_or_else(|| MmError::Unsupported("wgpu が D3D12 ではない".into()))?;
        queue.add_wait_fence(slot.fence.clone(), slot.copy_done_value());
        Ok(())
    }

    /// 枠を読む提出を終えたら、wgpu のキューに「読み終えた」値までフェンスを進めさせる。
    /// 提出のあとに呼ぶ。CUDA は次のコピーの前にこれを待つ。
    pub fn signal_read_done(&self, index: usize) -> Result<()> {
        let slot = &self.slots[index];
        // SAFETY: as_hal の参照はこの呼び出しの間だけ使う。
        let queue = unsafe { self.queue.as_hal::<wgpu::hal::api::Dx12>() }
            .ok_or_else(|| MmError::Unsupported("wgpu が D3D12 ではない".into()))?;
        // SAFETY: キューとフェンスは同じデバイスの有効なオブジェクト。
        unsafe { queue.as_raw().Signal(&slot.fence, slot.read_done_value()) }
            .map_err(|e| MmError::Internal(format!("フェンスの Signal: {e}")))
    }

    /// CUDA の取り込みを手放す。wgpu が読み終え、CUDA のコピーも終わっているときに呼ぶ。
    fn release_imports(&mut self) {
        // SAFETY: 文脈は復号器のもの。呼び出し側が GPU の仕事の完了を保証する。
        unsafe {
            if self.cuda.push(self.ctx).is_err() {
                return;
            }
            let _ = self.cuda.sync(self.stream);
            for slot in self.slots.drain(..) {
                self.cuda
                    .release_import(slot.cu_mem, slot.cu_ptr, slot.cu_sem);
            }
            let _ = self.cuda.pop();
        }
    }

    /// wgpu のデバイス。
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
}

impl Drop for SharedRing {
    fn drop(&mut self) {
        // wgpu 側の読み取りが残っていれば終わるまで待ってから CUDA の取り込みを外す。
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        self.release_imports();
    }
}

fn d3d_err(what: &str, e: windows::core::Error) -> MmError {
    MmError::Internal(format!("{what}: {e}"))
}

fn make_slot(device: &wgpu::Device, cuda: &CudaApi, size: u64) -> Result<Slot> {
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
        let desc = D3D12_RESOURCE_DESC {
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
        };
        let mut resource: Option<ID3D12Resource> = None;
        dev.CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_SHARED,
            &desc,
            D3D12_RESOURCE_STATE_COMMON,
            None,
            &mut resource,
        )
        .map_err(|e| d3d_err("共有バッファの作成", e))?;
        let resource = resource.ok_or_else(|| MmError::Internal("共有バッファが空".into()))?;
        let res_handle: HANDLE = dev
            .CreateSharedHandle(&resource, None, GENERIC_ALL, PCWSTR::null())
            .map_err(|e| d3d_err("共有ハンドル", e))?;
        let imported = cuda.import_buffer(res_handle.0, size);
        // CUDA が取り込み時に自前の参照を持つので、ハンドルは閉じる。
        let _ = CloseHandle(res_handle);
        let (cu_mem, cu_ptr) = imported?;

        let fence: ID3D12Fence = dev
            .CreateFence(0, D3D12_FENCE_FLAG_SHARED)
            .map_err(|e| d3d_err("共有フェンス", e))?;
        let fence_handle: HANDLE = dev
            .CreateSharedHandle(&fence, None, GENERIC_ALL, PCWSTR::null())
            .map_err(|e| d3d_err("フェンスの共有ハンドル", e))?;
        let imported = cuda.import_fence(fence_handle.0);
        let _ = CloseHandle(fence_handle);
        let cu_sem = imported?;

        let hal_buffer = wgpu::hal::dx12::Device::buffer_from_raw(resource, size);
        let buffer = device.create_buffer_from_hal::<wgpu::hal::api::Dx12>(
            hal_buffer,
            &wgpu::BufferDescriptor {
                label: Some("cuvid shared yuv420"),
                size,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            },
        );
        Ok(Slot {
            buffer,
            cu_ptr,
            fence,
            cu_sem,
            copies: 0,
            cu_mem,
        })
    }
}
