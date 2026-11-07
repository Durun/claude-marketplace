//! ホストがこのプロセスへ複製した D3D12 の共有テクスチャの NT ハンドルを開き、wgpu のテクスチャにする。
//! ハンドルは開いたらすぐ閉じる（資源はテクスチャが持つ）。形式は mm-host-cef の約束（BGRA8、COMMON）。

use mm_gpu::GpuContext;
use mm_types::{MmError, Result, Size};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Graphics::Direct3D12::ID3D12Resource;

/// 共有テクスチャの形式。
pub const SHARED_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// 開いたテクスチャ。コピーの元にだけ使う。コピーの後は状態を `PRESENT`（COMMON）に戻す。
pub fn open(ctx: &GpuContext, handle: u64, size: Size) -> Result<wgpu::Texture> {
    let resource = open_resource(ctx, handle)?;
    let extent = wgpu::Extent3d {
        width: size.width,
        height: size.height,
        depth_or_array_layers: 1,
    };
    let desc = wgpu::TextureDescriptor {
        label: Some("web.cef shared"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: SHARED_FORMAT,
        usage: wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    };
    // SAFETY: ホストは BGRA8 の 2D テクスチャを COMMON の状態で渡し、渡した後は書かない。
    let texture = unsafe {
        let hal = wgpu::hal::dx12::Device::texture_from_raw(
            resource,
            desc.format,
            desc.dimension,
            extent,
            1,
            1,
        );
        ctx.device.create_texture_from_hal::<wgpu::hal::api::Dx12>(
            hal,
            &desc,
            wgpu::TextureUses::PRESENT,
        )
    };
    Ok(texture)
}

fn open_resource(ctx: &GpuContext, handle: u64) -> Result<ID3D12Resource> {
    let raw = HANDLE(handle as usize as *mut _);
    // SAFETY: as_hal の参照はこのブロックの間だけ使う。raw はホストがこのプロセスへ複製したハンドルで、ほかに使う所は無い。
    unsafe {
        let opened = match ctx.device.as_hal::<wgpu::hal::api::Dx12>() {
            Some(hal) => {
                let mut resource: Option<ID3D12Resource> = None;
                hal.raw_device()
                    .OpenSharedHandle(raw, &mut resource)
                    .map(|()| resource)
                    .map_err(|e| format!("共有ハンドルを開けない: {e}"))
            }
            None => Err("D3D12 のデバイスでない".to_string()),
        };
        let _ = CloseHandle(raw);
        opened
            .map_err(MmError::Unsupported)?
            .ok_or_else(|| MmError::Internal("共有ハンドルの資源が空".into()))
    }
}

/// 使わずに捨てる応答のハンドルを閉じる。
pub fn close(handle: u64) {
    // SAFETY: ホストがこのプロセスへ複製したハンドルで、ほかに使う所は無い。
    let _ = unsafe { CloseHandle(HANDLE(handle as usize as *mut _)) };
}
