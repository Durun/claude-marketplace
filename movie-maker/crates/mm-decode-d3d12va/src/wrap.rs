//! FFmpeg の `ID3D12Resource`（NV12 か P010）を wgpu のテクスチャに包む。コピーはしない。

use std::collections::HashMap;
use std::ffi::c_void;

use ffmpeg_next::frame;
use mm_types::{MmError, Result};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::{ID3D12Fence, ID3D12Resource};

use mm_decode_hw::device::queue_wait;

use crate::ffi::{AvD3d12vaFrame, DXGI_FORMAT_NV12, DXGI_FORMAT_P010};

/// 包んだフレーム。`y` と `uv` が 2 つの面の見え方。8 bit は `R8Unorm` と `Rg8Unorm`、
/// 10 bit は `R16Unorm` と `Rg16Unorm`。
#[derive(Clone)]
pub struct PlaneViews {
    pub y: wgpu::TextureView,
    pub uv: wgpu::TextureView,
    /// デコードの完了を知らせるフェンスと値。
    pub fence: ID3D12Fence,
    pub fence_value: u64,
}

impl PlaneViews {
    /// キューの以後の実行を、デコードの完了まで GPU の側で待たせる（CPU は待たない）。
    /// 待ちはキューへ直接積むので、別のスレッドの submit が挟まっても変換より前に効く。
    pub fn wait_decode_on(&self, queue: &wgpu::Queue) -> Result<()> {
        queue_wait(queue, &self.fence, self.fence_value)
    }
}

/// FFmpeg の `hw_frames_ctx`（面のプール）への参照。プールが作り直されたかを、アドレスの再利用に
/// 惑わされず見分けるために、参照を持って生かしておく。
struct FramesRef(*mut ffmpeg_next::ffi::AVBufferRef);

// SAFETY: 参照の数えは FFmpeg が行う。持つのは 1 つの Wrapper だけ。
unsafe impl Send for FramesRef {}

impl FramesRef {
    /// `ctx` の AVBuffer（プールの実体）が同じか。
    fn same_pool(&self, ctx: *mut ffmpeg_next::ffi::AVBufferRef) -> bool {
        // SAFETY: どちらも有効な AVBufferRef。
        unsafe { (*self.0).buffer == (*ctx).buffer }
    }
}

impl Drop for FramesRef {
    fn drop(&mut self) {
        // SAFETY: av_buffer_ref で取った参照。
        unsafe { ffmpeg_next::ffi::av_buffer_unref(&mut self.0) };
    }
}

/// 資源ごとの wgpu のテクスチャと見え方。FFmpeg のプールは同じ資源を使い回すので、包むのは資源ごとに 1 回。
struct Entry {
    texture: wgpu::Texture,
    views: HashMap<u32, (wgpu::TextureView, wgpu::TextureView)>,
    layers: u32,
    bits: u32,
}

/// 包む係。
pub struct Wrapper {
    device: wgpu::Device,
    cache: HashMap<usize, Entry>,
    /// 覚えが対応する面のプール。変わったら覚えを捨てる。
    pool: Option<FramesRef>,
}

impl Wrapper {
    /// 空の係。
    pub fn new(device: wgpu::Device) -> Self {
        Self {
            device,
            cache: HashMap::new(),
            pool: None,
        }
    }

    /// フレームのプールが前と違えば、古いプールの面の覚えを捨てる（面は覚えが握っていて、捨てないと残り続ける）。
    fn follow_pool(&mut self, frames_ctx: *mut ffmpeg_next::ffi::AVBufferRef) {
        if frames_ctx.is_null() || self.pool.as_ref().is_some_and(|p| p.same_pool(frames_ctx)) {
            return;
        }
        self.cache.clear();
        // SAFETY: フレームが持つ有効な hw_frames_ctx。参照を足して、フレームより長く持つ。
        let held = unsafe { ffmpeg_next::ffi::av_buffer_ref(frames_ctx) };
        self.pool = (!held.is_null()).then_some(FramesRef(held));
    }

    /// 包んだ資源の数。
    pub fn resource_count(&self) -> usize {
        self.cache.len()
    }

    /// d3d12va のフレームを包む。
    pub fn wrap(&mut self, frame: &frame::Video) -> Result<PlaneViews> {
        // SAFETY: d3d12va のフレームは data[0] に AVD3D12VAFrame を持つ。フレームが生きている間だけ読む。
        let (resource_ptr, layer, fence_ptr, fence_value, frames_ctx) = unsafe {
            let f = frame.as_ptr();
            if (*f).format != ffmpeg_next::ffi::AVPixelFormat::AV_PIX_FMT_D3D12 as i32 {
                return Err(MmError::Unsupported("d3d12 のフレームではない".into()));
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
                (*f).hw_frames_ctx,
            )
        };
        self.follow_pool(frames_ctx);
        let key = resource_ptr as usize;
        if !self.cache.contains_key(&key) {
            let entry = self.import(resource_ptr)?;
            self.cache.insert(key, entry);
        }
        let entry = self.cache.get_mut(&key).expect("いま入れた資源");
        if layer >= entry.layers {
            return Err(MmError::Corrupt(format!("配列の番号 {layer} が範囲外")));
        }
        let bits = entry.bits;
        if !entry.views.contains_key(&layer) {
            let views = make_views(&entry.texture, layer, bits);
            entry.views.insert(layer, views);
        }
        let (y, uv) = entry.views[&layer].clone();
        // SAFETY: fence は FFmpeg が所有する有効な ID3D12Fence。参照を数え直して借りる。
        let fence = unsafe { ID3D12Fence::from_raw_borrowed(&fence_ptr) }
            .ok_or_else(|| MmError::Corrupt("フェンスが無い".into()))?
            .clone();
        Ok(PlaneViews {
            y,
            uv,
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
        let (format, bits) = match desc.Format.0 as u32 {
            DXGI_FORMAT_NV12 => (wgpu::TextureFormat::NV12, 8),
            DXGI_FORMAT_P010 => (wgpu::TextureFormat::P010, 10),
            other => {
                return Err(MmError::Unsupported(format!(
                    "NV12 と P010 以外の資源: DXGI {other}"
                )))
            }
        };
        let layers = u32::from(desc.DepthOrArraySize);
        let size = wgpu::Extent3d {
            width: desc.Width as u32,
            height: desc.Height,
            depth_or_array_layers: layers,
        };
        let wgpu_desc = wgpu::TextureDescriptor {
            label: Some("d3d12va yuv420"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        // SAFETY: 資源は wgpu と同じ ID3D12Device の上にあり、2D で、中身はデコード済み。
        // 同時アクセスのフラグ付きなので、暗黙の昇格で SRV として読める（初期状態は RESOURCE）。
        let texture = unsafe {
            let hal = wgpu::hal::dx12::Device::texture_from_raw(
                resource,
                format,
                wgpu::TextureDimension::D2,
                size,
                1,
                1,
            );
            self.device.create_texture_from_hal::<wgpu::hal::api::Dx12>(
                hal,
                &wgpu_desc,
                wgpu::TextureUses::RESOURCE,
            )
        };
        Ok(Entry {
            texture,
            views: HashMap::new(),
            layers,
            bits,
        })
    }
}

fn make_views(
    texture: &wgpu::Texture,
    layer: u32,
    bits: u32,
) -> (wgpu::TextureView, wgpu::TextureView) {
    let (luma, chroma) = if bits == 8 {
        (wgpu::TextureFormat::R8Unorm, wgpu::TextureFormat::Rg8Unorm)
    } else {
        (
            wgpu::TextureFormat::R16Unorm,
            wgpu::TextureFormat::Rg16Unorm,
        )
    };
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
        view(luma, wgpu::TextureAspect::Plane0),
        view(chroma, wgpu::TextureAspect::Plane1),
    )
}
