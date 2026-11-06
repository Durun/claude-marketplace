//! 画像を入力に取る種類（効果、トランジション）をレンダラーが呼ぶ仕組み。
//!
//! 入力は GPU のテクスチャ。種類は自前のコマンドを作ってすぐ提出するので、入力の中身がまだレンダラーのコマンドの
//! 中にあるうちは読めない。呼ぶ前に積んだコマンドを提出する（[`WgpuRenderer::flush`]）。

use std::panic::AssertUnwindSafe;

use mm_doc::Layer;
use mm_eval::node_params_at;
use mm_node_api::{FrameCx, FrameRef, GpuHelper, GpuTexture, InputFrames, NodeKind};
use mm_types::{MmError, Params, RationalTime, Result, Size};

use crate::layers::{CompCx, LayerImage};
use crate::renderer::WgpuRenderer;

/// 入力の列を、時刻によらず同じフレームで返す。
struct FrameList(Vec<FrameRef>);

impl InputFrames for FrameList {
    fn frame(&mut self, index: usize, _t: RationalTime) -> Result<FrameRef> {
        self.0
            .get(index)
            .cloned()
            .ok_or_else(|| MmError::NotFound(format!("入力 {index} は無い")))
    }

    fn len(&self) -> usize {
        self.0.len()
    }
}

/// 種類を呼ぶ 1 回分の入力。
pub(crate) struct NodeRun<'a> {
    pub kind: &'a dyn NodeKind,
    pub params: &'a Params,
    /// 入力のテクスチャ。順に入力 0、1 …。
    pub inputs: &'a [GpuTexture],
    /// 出力の大きさ（画素）。
    pub size: Size,
}

impl WgpuRenderer {
    /// ここまでに積んだコマンドを提出し、空のコマンドに差し替える。
    pub(crate) fn flush(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let ctx = self.gpu.context().clone();
        let fresh = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render.flush"),
            });
        let done = std::mem::replace(encoder, fresh);
        self.gpu.before_submit();
        ctx.queue.submit([done.finish()]);
        // 提出した後なので、それまでに手放した中間のテクスチャを使い回してよい。
        self.gpu.recycle();
    }

    /// 入力のテクスチャを与えて種類を呼び、出力のテクスチャの番号を返す。透明なら `None`。
    /// 恒等の種類は入力 0 の番号をそのまま返す。
    pub(crate) fn run_node(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        cx: &CompCx<'_>,
        run: &NodeRun<'_>,
    ) -> Result<Option<u64>> {
        self.flush(encoder);
        let Self { gpu, services, .. } = self;
        let mut inputs = FrameList(run.inputs.iter().cloned().map(FrameRef::Gpu).collect());
        let mut frame_cx = FrameCx {
            time: cx.t,
            params: run.params,
            size: run.size,
            region: run.size.to_rect(),
            scale: cx.size.width as f32 / cx.comp.width as f32,
            quality: cx.quality,
            inputs: &mut inputs,
            footage: None,
            source_time: cx.t,
            gpu: &mut *gpu,
            services,
        };
        if let (true, Some(first)) = (run.kind.is_identity(&frame_cx), run.inputs.first()) {
            return Ok(Some(first.id));
        }
        let rendered =
            std::panic::catch_unwind(AssertUnwindSafe(|| run.kind.render(&mut frame_cx)))
                .map_err(|_| MmError::Internal(format!("{} が panic", run.kind.id())))??;
        match rendered {
            FrameRef::Transparent { .. } => Ok(None),
            FrameRef::Gpu(tex) => Ok(Some(tex.id)),
            FrameRef::Cpu(image) => match gpu.upload(&image)? {
                FrameRef::Gpu(tex) => Ok(Some(tex.id)),
                _ => Err(MmError::Internal("upload が Gpu を返さない".into())),
            },
        }
    }

    /// レイヤーの効果を上から順に掛ける。作ったテクスチャは `temps` に積む（呼んだ側が解放する）。
    /// 効果が画像を透明にしたら `None`。
    pub(crate) fn apply_effects(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        cx: &CompCx<'_>,
        layer: &Layer,
        image: LayerImage,
        temps: &mut Vec<u64>,
    ) -> Result<Option<LayerImage>> {
        let mut image = image;
        for effect in layer.effects.iter().filter(|e| e.enabled) {
            let kind = cx
                .nodes
                .get(&effect.node.kind)
                .ok_or_else(|| MmError::NotFound(format!("効果の種類 {}", effect.node.kind)))?;
            let params = node_params_at(&effect.node, cx.t).with_defaults(&kind.params());
            let input = GpuTexture {
                id: image.texture,
                size: image.size,
            };
            let run = NodeRun {
                kind: kind.as_ref(),
                params: &params,
                inputs: &[input],
                size: image.size,
            };
            let Some(out) = self.run_node(encoder, cx, &run)? else {
                return Ok(None);
            };
            if out != image.texture {
                temps.push(out);
            }
            image.texture = out;
        }
        Ok(Some(image))
    }
}
