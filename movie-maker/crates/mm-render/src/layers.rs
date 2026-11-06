//! レイヤーの評価。ノードの種類を呼んで画像を作り、変形の行列と一緒に合成の入力にする。
//!
//! ノードの種類が返す画像の置き方（レンダラーの決まり）:
//! - 画像の大きさが `FrameCx::size`（画布）と同じなら、画布の全体を覆う。
//! - そうでなければ `FrameCx::region` の矩形を覆う。レンダラーは `region_of_definition` を `region` に渡すので、
//!   領域だけを描いた画像（文字）も、素材のままの大きさの GPU のフレーム（撮影素材）も、ここで 1 回だけ拡縮する。

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use mm_doc::{Composition, Layer, Project, SEQUENCE_KIND};
use mm_eval::{evaluate_opacity, node_params_at, source_time};
use mm_node_api::{FrameCx, FrameRef, GpuHelper, NoInputs, NodeKind, NodeRegistry, Quality};
use mm_types::{CompositionId, Id, MmError, Params, RationalTime, Rect, Result, Size};

use crate::compose::DrawItem;
use crate::renderer::WgpuRenderer;
use crate::transform::{uv_to_ndc, LayerTransform};

/// 入れ子のコンポジションの種類の名前。ノードのインターフェースからプロジェクトのデータが見えないので、展開はレンダラーが行う。
pub const NESTED_KIND: &str = "composition";
/// 入れ子の種類がコンポジションの ID を持つパラメータの名前。
pub const NESTED_PARAM: &str = "composition";
/// 入れ子の深さの上限。循環した参照で止まらなくなるのを防ぐ。
pub(crate) const MAX_DEPTH: u32 = 8;
/// ベクターを描き直す倍率の上限。
const MAX_VECTOR_SCALE: f32 = 4.0;
/// テクスチャの一辺の上限（wgpu の既定の 2D の上限）。
const MAX_TEXTURE_SIDE: u32 = 8192;

/// 1 つのコンポジションを 1 つの時刻で描くための入力。
#[derive(Clone, Copy)]
pub(crate) struct CompCx<'a> {
    pub project: &'a Project,
    pub nodes: &'a dyn NodeRegistry,
    pub quality: Quality,
    pub comp: &'a Composition,
    pub t: RationalTime,
    /// 描く先の画素の大きさ。
    pub size: Size,
    pub depth: u32,
}

impl CompCx<'_> {
    /// コンポジションの画素から描く先の画素への倍率。
    fn comp_to_out(&self) -> f32 {
        self.size.width as f32 / self.comp.width as f32
    }

    fn comp_size(&self) -> (f32, f32) {
        (self.comp.width as f32, self.comp.height as f32)
    }
}

/// 合成に渡す 1 枚。
pub(crate) struct Prepared {
    texture: u64,
    matrix: [f32; 16],
    opacity: f32,
    taps: [f32; 2],
}

/// レイヤーの画像のテクスチャと、コンポジションの画素での置き場所。
pub(crate) struct LayerImage {
    pub(crate) texture: u64,
    pub(crate) size: Size,
    pub(crate) place: Rect,
}

/// レイヤーの種類と、その時刻のパラメータ（既定値で埋めたもの）。
pub(crate) fn evaluate(
    nodes: &dyn NodeRegistry,
    layer: &Layer,
    t: RationalTime,
) -> Result<(Arc<dyn NodeKind>, Params)> {
    let kind = nodes
        .get(&layer.source.kind)
        .ok_or_else(|| MmError::NotFound(format!("ノードの種類 {}", layer.source.kind)))?;
    let params = node_params_at(&layer.source, t).with_defaults(&kind.params());
    Ok((kind, params))
}

impl WgpuRenderer {
    /// コンポジションを新しいテクスチャに描くパスを `encoder` に積む。返すテクスチャは呼んだ側がドロップする。
    pub(crate) fn encode_comp(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        cx: &CompCx<'_>,
    ) -> Result<u64> {
        let target = self.gpu.create(cx.size)?;
        let mut prepared = Vec::new();
        let mut temps = Vec::new();
        for layer in &cx.comp.layers {
            if !layer.range()?.contains(cx.t) {
                continue;
            }
            let _span = tracing::debug_span!("render.layer").entered();
            match self.layer_item(encoder, cx, layer, &mut temps) {
                Ok(Some(p)) => prepared.push(p),
                Ok(None) => {}
                Err(e) => self.errors.push(format!("layer {}: {e}", layer.id)),
            }
        }
        self.composite(encoder, target, &prepared);
        mm_trace::counter!("render.layers_drawn", prepared.len());
        // 操作がテクスチャを握っているので、提出の前にプールから外してよい。
        for id in temps {
            self.gpu.release_id(id);
        }
        Ok(target)
    }

    pub(crate) fn composite(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: u64,
        prepared: &[Prepared],
    ) {
        let _span = tracing::debug_span!("render.composite").entered();
        let items: Vec<DrawItem<'_>> = prepared
            .iter()
            .filter_map(|p| {
                Some(DrawItem {
                    texture: self.gpu.texture(p.texture)?,
                    matrix: p.matrix,
                    opacity: p.opacity,
                    taps: p.taps,
                })
            })
            .collect();
        let target_texture = self.gpu.texture(target).expect("作ったばかり");
        self.compositor.draw(
            self.gpu.context(),
            encoder,
            target_texture,
            &items,
            self.timer.pass_writes(),
        );
    }

    pub(crate) fn layer_item(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        cx: &CompCx<'_>,
        layer: &Layer,
        temps: &mut Vec<u64>,
    ) -> Result<Option<Prepared>> {
        let (kind, params) = evaluate(cx.nodes, layer, cx.t)?;
        let transform = LayerTransform::from_doc(&layer.transform, cx.t);
        let image = match kind.id() {
            NESTED_KIND => self.nested_image(encoder, cx, layer, &params, &transform)?,
            SEQUENCE_KIND => self.sequence_image(encoder, cx, layer, &params)?,
            _ => self.kind_image(cx, kind.as_ref(), layer, &params, &transform)?,
        };
        let Some(image) = image else {
            return Ok(None);
        };
        temps.push(image.texture);
        let Some(image) = self.apply_effects(encoder, cx, layer, image, temps)? else {
            return Ok(None);
        };
        let opacity = evaluate_opacity(layer, cx.t) as f32;
        let out = (cx.size.width as f32, cx.size.height as f32);
        Ok(Some(Prepared {
            texture: image.texture,
            matrix: uv_to_ndc(
                &transform,
                image.place,
                cx.comp_size(),
                cx.comp_to_out(),
                out,
            ),
            opacity,
            taps: minify_taps(&image, &transform, cx.comp_to_out()),
        }))
    }

    /// ノードの種類を呼んで、画像を GPU のテクスチャにする。透明か領域が空なら `None`。
    fn kind_image(
        &mut self,
        cx: &CompCx<'_>,
        kind: &dyn NodeKind,
        layer: &Layer,
        params: &Params,
        transform: &LayerTransform,
    ) -> Result<Option<LayerImage>> {
        let Self {
            sources,
            decoders,
            gpu,
            services,
            ..
        } = self;
        let footage = sources.footage_for(
            decoders.as_ref(),
            gpu.context().as_ref(),
            cx.project,
            params,
        )?;
        let mut inputs = NoInputs;
        let mut frame_cx = FrameCx {
            time: cx.t,
            params,
            size: cx.size,
            region: cx.size.to_rect(),
            scale: cx.comp_to_out(),
            quality: cx.quality,
            inputs: &mut inputs,
            footage,
            source_time: source_time(layer, cx.t)?,
            gpu: &mut *gpu,
            services,
        };
        let rod = kind.region_of_definition(&frame_cx);
        if rod.is_empty() {
            return Ok(None);
        }
        // ベクターは拡大する分だけ細かく描き、後から拡大しない。倍率は領域の大きさだけに掛かる。
        let boost = if kind.traits().vector {
            vector_boost(transform, &rod)
        } else {
            1.0
        };
        frame_cx.size = scaled(cx.size, boost);
        frame_cx.region = scaled_rect(&rod, boost);
        frame_cx.scale *= boost;
        let canvas = frame_cx.size;
        let rendered = std::panic::catch_unwind(AssertUnwindSafe(|| kind.render(&mut frame_cx)))
            .map_err(|_| MmError::Internal(format!("{} が panic", kind.id())))??;
        let image_size = rendered.size();
        let texture = match rendered {
            FrameRef::Transparent { .. } => return Ok(None),
            FrameRef::Cpu(image) => match gpu.upload(&image)? {
                FrameRef::Gpu(tex) => tex.id,
                _ => return Err(MmError::Internal("upload が Gpu を返さない".into())),
            },
            FrameRef::Gpu(tex) => tex.id,
        };
        let place = if image_size == canvas {
            Rect::new(0.0, 0.0, cx.comp.width as f32, cx.comp.height as f32)
        } else {
            scaled_rect(&rod, 1.0 / cx.comp_to_out())
        };
        Ok(Some(LayerImage {
            texture,
            size: image_size,
            place,
        }))
    }

    /// 入れ子のコンポジションを別のテクスチャに描く。中にベクターがあるときは、入れ子のレイヤーを
    /// 拡大する分だけ大きく描き、中のベクターの倍率が親の拡大を含むようにする。
    fn nested_image(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        cx: &CompCx<'_>,
        layer: &Layer,
        params: &Params,
        transform: &LayerTransform,
    ) -> Result<Option<LayerImage>> {
        let name = params
            .get(NESTED_PARAM)
            .and_then(|v| v.as_text())
            .unwrap_or("");
        if name.is_empty() {
            return Ok(None);
        }
        if cx.depth >= MAX_DEPTH {
            return Err(MmError::InvalidArgument(format!(
                "入れ子が {MAX_DEPTH} 段を超えた（循環の疑い）"
            )));
        }
        let id: CompositionId = Id::new(name);
        let nested = cx.project.composition(&id)?;
        let boost = if has_vector(cx, nested, cx.depth + 1) {
            nested_boost(transform, cx, nested)
        } else {
            1.0
        };
        let child = CompCx {
            comp: nested,
            t: source_time(layer, cx.t)?,
            size: scaled(
                Size::new(nested.width, nested.height),
                cx.comp_to_out() * boost,
            ),
            depth: cx.depth + 1,
            ..*cx
        };
        let texture = self.encode_comp(encoder, &child)?;
        Ok(Some(LayerImage {
            texture,
            size: child.size,
            place: Rect::new(0.0, 0.0, nested.width as f32, nested.height as f32),
        }))
    }
}

/// 出力の 1 画素あたりの元の画素の数（軸ごと）。1 を超える軸は合成で面積平均に縮小する。
fn minify_taps(image: &LayerImage, transform: &LayerTransform, comp_to_out: f32) -> [f32; 2] {
    let out_w = image.place.width * comp_to_out * transform.scale[0].abs();
    let out_h = image.place.height * comp_to_out * transform.scale[1].abs();
    [
        image.size.width as f32 / out_w.max(f32::EPSILON),
        image.size.height as f32 / out_h.max(f32::EPSILON),
    ]
}

/// ベクターの種類を描く倍率。拡大は `MAX_VECTOR_SCALE` までで、領域のテクスチャの一辺が上限を超えない。
fn vector_boost(transform: &LayerTransform, rod: &Rect) -> f32 {
    let side = rod.width.max(rod.height).max(1.0);
    let room = MAX_TEXTURE_SIDE as f32 / side;
    transform
        .max_scale()
        .clamp(1.0, MAX_VECTOR_SCALE)
        .min(room.max(1.0))
}

/// 入れ子を描く倍率。拡大は `MAX_VECTOR_SCALE` までで、テクスチャの一辺が上限を超えない。
fn nested_boost(transform: &LayerTransform, cx: &CompCx<'_>, nested: &Composition) -> f32 {
    let base = cx.comp_to_out() * nested.width.max(nested.height) as f32;
    let room = MAX_TEXTURE_SIDE as f32 / base.max(1.0);
    transform
        .max_scale()
        .clamp(1.0, MAX_VECTOR_SCALE)
        .min(room.max(1.0))
}

/// コンポジションの中（入れ子の先も）にベクターの種類があるか。
fn has_vector(cx: &CompCx<'_>, comp: &Composition, depth: u32) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    comp.layers.iter().any(|layer| {
        let Ok((kind, params)) = evaluate(cx.nodes, layer, cx.t) else {
            return false;
        };
        if kind.id() != NESTED_KIND {
            return kind.traits().vector;
        }
        let child: CompositionId = Id::new(
            params
                .get(NESTED_PARAM)
                .and_then(|v| v.as_text())
                .unwrap_or(""),
        );
        cx.project
            .composition(&child)
            .is_ok_and(|c| has_vector(cx, c, depth + 1))
    })
}

fn scaled(size: Size, k: f32) -> Size {
    Size::new(
        (size.width as f32 * k).round().max(1.0) as u32,
        (size.height as f32 * k).round().max(1.0) as u32,
    )
}

fn scaled_rect(rect: &Rect, k: f32) -> Rect {
    Rect::new(rect.x * k, rect.y * k, rect.width * k, rect.height * k)
}
