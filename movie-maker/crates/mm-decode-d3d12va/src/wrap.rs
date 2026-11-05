//! FFmpeg の `ID3D12Resource`（NV12）を wgpu のテクスチャに包む。コピーはしない。

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Arc;

use ffmpeg_next::frame;
use mm_types::{MmError, Result};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::{ID3D12Fence, ID3D12Resource};

use crate::ffi::{AvD3d12vaFrame, DXGI_FORMAT_NV12};
use crate::gpu::GpuContext;

/// 包んだコマ。`y` と `uv` が NV12 の 2 つの面の見え方。
#[derive(Clone)]
pub struct GpuNv12 {
    pub y: wgpu::TextureView,
    pub uv: wgpu::TextureView,
    /// コマの大きさ（資源の大きさは符号化の境界で大きいことがある）。
    pub width: u32,
    pub height: u32,
    /// 復号の完了を知らせるフェンスと値。
    fence: ID3D12Fence,
    fence_value: u64,
}

impl GpuNv12 {
    /// 次の `Queue::submit` の頭で、復号の完了までキューを待たせる（CPU は待たない）。
    pub fn wait_decode_on(&self, gpu: &GpuContext) -> Result<()> {
        // SAFETY: as_hal の参照はこの呼び出しの間だけ使う。
        let queue = unsafe { gpu.queue.as_hal::<wgpu::hal::api::Dx12>() }
            .ok_or_else(|| MmError::Unsupported("wgpu が D3D12 ではない".into()))?;
        queue.add_wait_fence(self.fence.clone(), self.fence_value);
        Ok(())
    }
}

/// 資源ごとの wgpu のテクスチャと見え方。FFmpeg のプールは同じ資源を使い回すので、包むのは資源ごとに 1 回。
struct Entry {
    texture: wgpu::Texture,
    views: HashMap<u32, (wgpu::TextureView, wgpu::TextureView)>,
    layers: u32,
}

/// 包む係。
pub struct Wrapper {
    gpu: Arc<GpuContext>,
    cache: HashMap<usize, Entry>,
}

impl Wrapper {
    /// 空の係。
    pub fn new(gpu: Arc<GpuContext>) -> Self {
        Self {
            gpu,
            cache: HashMap::new(),
        }
    }

    /// 包んだ資源の数。
    pub fn resource_count(&self) -> usize {
        self.cache.len()
    }

    /// d3d12va のコマを包む。
    pub fn wrap(&mut self, frame: &frame::Video) -> Result<GpuNv12> {
        // SAFETY: d3d12va のコマは data[0] に AVD3D12VAFrame を持つ。コマが生きている間だけ読む。
        let (resource_ptr, layer, fence_ptr, fence_value) = unsafe {
            let f = frame.as_ptr();
            if (*f).format != ffmpeg_next::ffi::AVPixelFormat::AV_PIX_FMT_D3D12 as i32 {
                return Err(MmError::Unsupported("d3d12 のコマではない".into()));
            }
            let d = (*f).data[0] as *const AvD3d12vaFrame;
            if d.is_null() {
                return Err(MmError::Corrupt("AVD3D12VAFrame が無い".into()));
            }
            (
                (*d).texture,
                (*d).subresource_index as u32,
                (*d).sync_ctx.fence,
                (*d).sync_ctx.fence_value,
            )
        };
        let key = resource_ptr as usize;
        if !self.cache.contains_key(&key) {
            let entry = self.import(resource_ptr)?;
            self.cache.insert(key, entry);
        }
        let entry = self.cache.get_mut(&key).expect("いま入れた資源");
        if layer >= entry.layers {
            return Err(MmError::Corrupt(format!("配列の番号 {layer} が範囲外")));
        }
        if !entry.views.contains_key(&layer) {
            let views = make_views(&entry.texture, layer);
            entry.views.insert(layer, views);
        }
        let (y, uv) = entry.views[&layer].clone();
        // SAFETY: fence は FFmpeg が所有する有効な ID3D12Fence。参照を数え直して借りる。
        let fence = unsafe { ID3D12Fence::from_raw_borrowed(&fence_ptr) }
            .ok_or_else(|| MmError::Corrupt("フェンスが無い".into()))?
            .clone();
        Ok(GpuNv12 {
            y,
            uv,
            width: frame.width(),
            height: frame.height(),
            fence,
            fence_value,
        })
    }

    fn import(&self, resource_ptr: *mut c_void) -> Result<Entry> {
        // SAFETY: resource_ptr は FFmpeg が所有する有効な ID3D12Resource。AddRef して wgpu に渡す。
        let resource: ID3D12Resource = unsafe { ID3D12Resource::from_raw_borrowed(&resource_ptr) }
            .ok_or_else(|| MmError::Corrupt("資源が無い".into()))?
            .clone();
        // SAFETY: 引数なしの問い合わせ。
        let desc = unsafe { resource.GetDesc() };
        if desc.Format.0 as u32 != DXGI_FORMAT_NV12 {
            return Err(MmError::Unsupported(format!(
                "NV12 ではない資源: DXGI {}",
                desc.Format.0
            )));
        }
        let layers = u32::from(desc.DepthOrArraySize);
        let size = wgpu::Extent3d {
            width: desc.Width as u32,
            height: desc.Height,
            depth_or_array_layers: layers,
        };
        let wgpu_desc = wgpu::TextureDescriptor {
            label: Some("d3d12va nv12"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::NV12,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        // SAFETY: 資源は wgpu と同じ ID3D12Device の上にあり、NV12 の 2D で、中身は復号済み。
        // 同時アクセスのフラグ付きなので、暗黙の昇格で SRV として読める（初期状態は RESOURCE）。
        let texture = unsafe {
            let hal = wgpu::hal::dx12::Device::texture_from_raw(
                resource,
                wgpu::TextureFormat::NV12,
                wgpu::TextureDimension::D2,
                size,
                1,
                1,
            );
            self.gpu
                .device
                .create_texture_from_hal::<wgpu::hal::api::Dx12>(
                    hal,
                    &wgpu_desc,
                    wgpu::TextureUses::RESOURCE,
                )
        };
        Ok(Entry {
            texture,
            views: HashMap::new(),
            layers,
        })
    }
}

fn make_views(texture: &wgpu::Texture, layer: u32) -> (wgpu::TextureView, wgpu::TextureView) {
    let view = |format, aspect| {
        texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("d3d12va plane"),
            format: Some(format),
            dimension: Some(wgpu::TextureViewDimension::D2),
            usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
            aspect,
            base_mip_level: 0,
            mip_level_count: Some(1),
            base_array_layer: layer,
            array_layer_count: Some(1),
        })
    };
    (
        view(wgpu::TextureFormat::R8Unorm, wgpu::TextureAspect::Plane0),
        view(wgpu::TextureFormat::Rg8Unorm, wgpu::TextureAspect::Plane1),
    )
}
