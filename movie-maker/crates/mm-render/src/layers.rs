//! レイヤーの評価。ノードの種類を呼んで画像を作り、変形の行列と一緒に合成の入力にする。

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use mm_doc::{Composition, Layer, Project};
use mm_eval::{evaluate_opacity, node_params_at, source_time};
use mm_node_api::{FrameCx, FrameRef, GpuHelper, NoInputs, NodeKind, NodeRegistry, Quality};
use mm_types::{CompositionId, Id, MmError, Params, RationalTime, Result, Size};

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
/// 入れ子のテクスチャの一辺の上限（wgpu の既定の 2D の上限）。
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
}

/// 合成に渡す 1 枚。
struct Prepared {
    texture: u64,
    matrix: [f32; 16],
    opacity: f32,
}

/// レイヤーの画像のテクスチャと、コンポジションの画素での大きさ。
struct LayerImage {
    texture: u64,
    logical: (f32, f32),
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

    fn composite(
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

    fn layer_item(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        cx: &CompCx<'_>,
        layer: &Layer,
        temps: &mut Vec<u64>,
    ) -> Result<Option<Prepared>> {
        let (kind, params) = evaluate(cx.nodes, layer, cx.t)?;
        let transform = LayerTransform::from_doc(&layer.transform, cx.t);
        let image = if kind.id() == NESTED_KIND {
            self.nested_image(encoder, cx, layer, &params, &transform)?
        } else {
            self.kind_image(cx, kind.as_ref(), layer, &params, &transform)?
        };
        let Some(image) = image else {
            return Ok(None);
        };
        temps.push(image.texture);
        let opacity = evaluate_opacity(layer, cx.t) as f32;
        let out = (cx.size.width as f32, cx.size.height as f32);
        Ok(Some(Prepared {
            texture: image.texture,
            matrix: uv_to_ndc(&transform, image.logical, cx.comp_to_out(), out),
            opacity,
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
        let boost = vector_boost(kind, transform);
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
        let size = scaled(cx.size, boost);
        let mut frame_cx = FrameCx {
            time: cx.t,
            params,
            size,
            region: size.to_rect(),
            scale: cx.comp_to_out() * boost,
            quality: cx.quality,
            inputs: &mut inputs,
            footage,
            source_time: source_time(layer, cx.t)?,
            gpu: &mut *gpu,
            services,
        };
        if kind.region_of_definition(&frame_cx).is_empty() {
            return Ok(None);
        }
        let rendered = std::panic::catch_unwind(AssertUnwindSafe(|| kind.render(&mut frame_cx)))
            .map_err(|_| MmError::Internal(format!("{} が panic", kind.id())))??;
        let texture = match rendered {
            FrameRef::Transparent { .. } => return Ok(None),
            FrameRef::Cpu(image) => match gpu.upload(&image)? {
                FrameRef::Gpu(tex) => tex.id,
                _ => return Err(MmError::Internal("upload が Gpu を返さない".into())),
            },
            FrameRef::Gpu(tex) => tex.id,
        };
        let logical = (cx.comp.width as f32, cx.comp.height as f32);
        Ok(Some(LayerImage { texture, logical }))
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
            logical: (nested.width as f32, nested.height as f32),
        }))
    }
}

/// ベクターの種類は、レイヤーを拡大する分だけ細かく描いて、後から拡大しない。
fn vector_boost(kind: &dyn NodeKind, transform: &LayerTransform) -> f32 {
    if kind.traits().vector {
        transform.max_scale().clamp(1.0, MAX_VECTOR_SCALE)
    } else {
        1.0
    }
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
