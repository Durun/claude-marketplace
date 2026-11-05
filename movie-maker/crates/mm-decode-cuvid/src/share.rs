//! CUDA と wgpu が共有する D3D12 のバッファの輪。cuvid のコマを 1 回コピーして wgpu に渡す。

use std::sync::Arc;

use ffmpeg_next::frame;
use mm_types::{MmError, Result};
use windows::core::PCWSTR;
use windows::Win32::Foundation::HANDLE;
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
use crate::gpu::GpuContext;

/// `GENERIC_ALL`。共有ハンドルのアクセス権。
const GENERIC_ALL: u32 = 0x1000_0000;

/// 輪の 1 枠。
pub struct Slot {
    pub buffer: wgpu::Buffer,
    cu_ptr: CuDevicePtr,
    fence: ID3D12Fence,
    cu_sem: CuExternalSemaphore,
    value: u64,
    _cu_mem: CuExternalMemory,
}

/// 共有バッファの輪。
pub struct SharedRing {
    gpu: Arc<GpuContext>,
    cuda: Arc<CudaApi>,
    ctx: CuContext,
    stream: CuStream,
    slots: Vec<Slot>,
    next: usize,
    width: u32,
    height: u32,
}

impl SharedRing {
    /// Y 面の後に UV 面が続く 1 本のバッファ（行の間に余白なし）を `count` 枠つくる。
    ///
    /// # Safety
    /// `ctx` と `stream` は、復号器が使う有効な CUDA の文脈とストリーム。
    pub unsafe fn new(
        gpu: Arc<GpuContext>,
        cuda: Arc<CudaApi>,
        ctx: CuContext,
        stream: CuStream,
        (width, height): (u32, u32),
        count: usize,
    ) -> Result<Self> {
        let size = u64::from(width) * u64::from(height) * 3 / 2;
        let size = size.next_multiple_of(4);
        // SAFETY: 文脈を現在にしている間だけ CUDA を呼ぶ。
        unsafe { cuda.push(ctx)? };
        let slots: Result<Vec<Slot>> = (0..count).map(|_| make_slot(&gpu, &cuda, size)).collect();
        // SAFETY: 上の push と対。
        unsafe { cuda.pop()? };
        Ok(Self {
            gpu,
            cuda,
            ctx,
            stream,
            slots: slots?,
            next: 0,
            width,
            height,
        })
    }

    /// UV 面の先頭のバイト位置。
    pub fn uv_offset(&self) -> u32 {
        self.width * self.height
    }

    /// 枠の数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 枠が無いか。
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 枠の中身。
    pub fn slot(&self, index: usize) -> &Slot {
        &self.slots[index]
    }

    /// cuvid のコマ（CUDA の NV12）を次の枠にコピーし、完了のフェンスを積む。CPU は待たない。
    /// 戻り値は枠の番号。
    pub fn copy_frame(&mut self, f: &frame::Video) -> Result<usize> {
        let index = self.next;
        self.next = (self.next + 1) % self.slots.len();
        let slot = &mut self.slots[index];
        slot.value += 1;
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
                    u64::from(self.width) * u64::from(self.height),
                ),
            ];
            self.cuda.push(self.ctx)?;
            for (src, pitch, rows, dst_offset) in planes {
                let copy = Memcpy2D {
                    src_x_in_bytes: 0,
                    src_y: 0,
                    src_memory_type: MEMORYTYPE_DEVICE,
                    src_host: std::ptr::null(),
                    src_device: src,
                    src_array: std::ptr::null_mut(),
                    src_pitch: pitch as usize,
                    dst_x_in_bytes: 0,
                    dst_y: 0,
                    dst_memory_type: MEMORYTYPE_DEVICE,
                    dst_host: std::ptr::null_mut(),
                    dst_device: slot.cu_ptr + dst_offset,
                    dst_array: std::ptr::null_mut(),
                    dst_pitch: self.width as usize,
                    width_in_bytes: self.width as usize,
                    height: rows as usize,
                };
                self.cuda.memcpy2d_async(&copy, self.stream)?;
            }
            self.cuda.signal(slot.cu_sem, slot.value, self.stream)?;
            self.cuda.pop()?;
        }
        Ok(index)
    }

    /// 次の `Queue::submit` の頭で、枠のコピーの完了までキューを待たせる（CPU は待たない）。
    pub fn wait_copy_on(&self, index: usize) -> Result<()> {
        let slot = &self.slots[index];
        // SAFETY: as_hal の参照はこの呼び出しの間だけ使う。
        let queue = unsafe { self.gpu.queue.as_hal::<wgpu::hal::api::Dx12>() }
            .ok_or_else(|| MmError::Unsupported("wgpu が D3D12 ではない".into()))?;
        queue.add_wait_fence(slot.fence.clone(), slot.value);
        Ok(())
    }
}

fn d3d_err(what: &str, e: windows::core::Error) -> MmError {
    MmError::Internal(format!("{what}: {e}"))
}

fn make_slot(gpu: &GpuContext, cuda: &CudaApi, size: u64) -> Result<Slot> {
    // SAFETY: as_hal の参照はこの関数の間だけ使う。資源は wgpu と同じデバイスに作る。
    unsafe {
        let hal = gpu
            .device
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
        let (cu_mem, cu_ptr) = cuda.import_buffer(res_handle.0, size)?;

        let fence: ID3D12Fence = dev
            .CreateFence(0, D3D12_FENCE_FLAG_SHARED)
            .map_err(|e| d3d_err("共有フェンス", e))?;
        let fence_handle: HANDLE = dev
            .CreateSharedHandle(&fence, None, GENERIC_ALL, PCWSTR::null())
            .map_err(|e| d3d_err("フェンスの共有ハンドル", e))?;
        let cu_sem = cuda.import_fence(fence_handle.0)?;

        let hal_buffer = wgpu::hal::dx12::Device::buffer_from_raw(resource, size);
        let buffer = gpu.device.create_buffer_from_hal::<wgpu::hal::api::Dx12>(
            hal_buffer,
            &wgpu::BufferDescriptor {
                label: Some("cuvid shared nv12"),
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
            value: 0,
            _cu_mem: cu_mem,
        })
    }
}
