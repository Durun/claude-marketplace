//! レイヤーの画像を、フレームのキャッシュで引いてから種類に描かせる所（`kind_image` の後半）。
//!
//! 外れたら種類を呼んで描き、返った画像をテクスチャにしてキャッシュに覚える。置き場所の決まりは [`crate::layers`] の冒頭。

use std::panic::AssertUnwindSafe;

use mm_gpu::WgpuGpu;
use mm_node_api::{FrameCx, FrameRef, GpuHelper, NodeKind};
use mm_types::{MmError, Rect, Result, Size};

use crate::frame_cache::{Keyed, RenderCache};
use crate::layers::LayerImage;

/// 種類が返した画像の置き場所の候補（コンポジションの画素）。
pub(crate) struct Placement {
    /// 種類に渡した画布の大きさ。
    pub canvas: Size,
    /// 画布と同じ大きさの画像が覆う、コンポジションの全体。
    pub comp: Rect,
    /// 領域だけを描いた画像が覆う矩形。
    pub region: Rect,
}

/// キーで引き、外れたら種類を呼んで描き、覚える。透明なら `None`。
pub(crate) fn render_cached(
    cache: &mut RenderCache,
    kind: &dyn NodeKind,
    frame_cx: &mut FrameCx<'_>,
    key: Option<Keyed>,
    place: &Placement,
) -> Result<Option<LayerImage>> {
    if let Some((k, deps)) = &key {
        if let Some(hit) = cache.lookup(k, deps, &mut *frame_cx.gpu) {
            return Ok(Some(hit.into()));
        }
    }
    let rendered = std::panic::catch_unwind(AssertUnwindSafe(|| kind.render(frame_cx)))
        .map_err(|_| MmError::Internal(format!("{} が panic", kind.id())))??;
    let gpu = frame_cx
        .gpu
        .as_any()
        .downcast_mut::<WgpuGpu>()
        .ok_or_else(|| MmError::Internal("レンダラーの GPU が wgpu でない".into()))?;
    let image_size = rendered.size();
    let Some((texture, owned)) = upload_rendered(gpu, rendered)? else {
        return Ok(None);
    };
    let image = LayerImage {
        texture,
        size: image_size,
        place: if image_size == place.canvas {
            place.comp
        } else {
            place.region
        },
        cached: false,
    };
    Ok(Some(
        match cache.remember(key, image.as_cached(), owned, gpu) {
            Some(kept) => kept.into(),
            None => image,
        },
    ))
}

/// 種類が返した画像をテクスチャにする。透明なら `None`。2 つ目は、テクスチャの持ち主がレンダラーか
/// （偽ならデコーダーの借用）。GPU で描く種類のテクスチャはヘルパーのプールのものなので、写さずに覚えられる。
fn upload_rendered(gpu: &mut WgpuGpu, rendered: FrameRef) -> Result<Option<(u64, bool)>> {
    Ok(match rendered {
        FrameRef::Transparent { .. } => None,
        FrameRef::Cpu(image) => match gpu.upload(&image)? {
            FrameRef::Gpu(tex) => Some((tex.id, true)),
            _ => return Err(MmError::Internal("upload が Gpu を返さない".into())),
        },
        FrameRef::Gpu(tex) => Some((tex.id, !gpu.is_borrowed(tex.id))),
    })
}
