//! wgpu の描画器。`Renderer` の実装。

use std::collections::VecDeque;
use std::sync::Arc;

use mm_audio_api::{AudioBlock, MixerFactory};
use mm_decode_api::DecoderFactory;
use mm_node_api::{FrameRef, GpuHelper, GpuTexture, Services};
use mm_render_api::{AudioRequest, RangeRequest, RenderRequest, Renderer};
use mm_types::{CompositionId, RationalTime, Result, TimeRange};

use crate::compose::Compositor;
use crate::layers::CompCx;
use crate::source::SourcePool;
use crate::timer::GpuTimer;
use mm_gpu::{GpuContext, WgpuGpu};

/// 描画器の版。合成の式を変えたら上げる（コマのキャッシュのキーに入る）。
pub const WGPU_RENDERER_VERSION: u32 = 1;
/// `render_frame` が返したテクスチャを持っておく数。これより古いコマの `FrameRef` は無効になる。
const KEPT_OUTPUTS: usize = 8;

/// wgpu の描画器。
pub struct WgpuRenderer {
    pub(crate) gpu: WgpuGpu,
    pub(crate) compositor: Compositor,
    pub(crate) timer: GpuTimer,
    pub(crate) decoders: Arc<dyn DecoderFactory>,
    pub(crate) mixers: Arc<dyn MixerFactory>,
    pub(crate) services: Services,
    pub(crate) sources: SourcePool,
    pub(crate) errors: Vec<String>,
    outputs: VecDeque<u64>,
}

impl WgpuRenderer {
    /// 文脈と口を受け取って組む。
    pub fn new(
        ctx: Arc<GpuContext>,
        decoders: Arc<dyn DecoderFactory>,
        mixers: Arc<dyn MixerFactory>,
        services: Services,
    ) -> Self {
        Self {
            compositor: Compositor::new(&ctx),
            timer: GpuTimer::new(&ctx),
            gpu: WgpuGpu::new(ctx),
            decoders,
            mixers,
            services,
            sources: SourcePool::default(),
            errors: Vec::new(),
            outputs: VecDeque::new(),
        }
    }

    /// 透明にしたレイヤーのエラーを取り出す（印の元）。
    pub fn take_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.errors)
    }

    /// 持っているテクスチャの数。漏れの検査に使う。
    pub fn texture_count(&self) -> usize {
        self.gpu.texture_count()
    }

    /// 前のコマの GPU の時間を読んで `mm-trace` に載せる。
    ///
    /// GPU の時間はスパンの開閉で測れないため、測った長さをそのままスパン `render.gpu_frame` の
    /// 分布に載せ、直近の値をゲージ `render.gpu_frame_us` にも出す。
    fn report_gpu_time(&mut self) {
        let Some(ns) = self.timer.collect() else {
            return;
        };
        mm_trace::record_span("render.gpu_frame", std::time::Duration::from_nanos(ns));
        mm_trace::gauge!("render.gpu_frame_us", ns / 1_000);
    }

    fn keep_output(&mut self, id: u64) {
        self.outputs.push_back(id);
        while self.outputs.len() > KEPT_OUTPUTS {
            if let Some(old) = self.outputs.pop_front() {
                self.gpu.release_id(old);
            }
        }
    }

    fn draw_frame(&mut self, req: &RenderRequest<'_>) -> Result<u64> {
        self.report_gpu_time();
        self.timer.begin_frame();
        let comp = req.project.composition(req.composition)?;
        let cx = CompCx {
            project: req.project,
            nodes: req.nodes,
            quality: req.quality,
            comp,
            t: req.time,
            size: req.size,
            depth: 0,
        };
        let ctx = self.gpu.context().clone();
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render.frame"),
            });
        let target = self.encode_comp(&mut encoder, &cx);
        self.timer.resolve(&mut encoder);
        // 借りた GPU のコマは、この提出が終わるまでの借り物（RFC 0001）。前に待ち、後に返す。
        self.gpu.before_submit();
        ctx.queue.submit([encoder.finish()]);
        self.gpu.after_submit();
        target
    }
}

impl Renderer for WgpuRenderer {
    fn id(&self) -> &'static str {
        "wgpu"
    }

    fn version(&self) -> u32 {
        WGPU_RENDERER_VERSION
    }

    fn render_frame(&mut self, req: &RenderRequest<'_>) -> Result<FrameRef> {
        let _span = tracing::info_span!("render.frame", comp = %req.composition).entered();
        let id = self.draw_frame(req)?;
        self.keep_output(id);
        Ok(FrameRef::Gpu(GpuTexture { id, size: req.size }))
    }

    fn render_range(
        &mut self,
        req: &RangeRequest<'_>,
        sink: &mut dyn FnMut(i64, FrameRef, &mut dyn GpuHelper) -> Result<()>,
    ) -> Result<usize> {
        let _span = tracing::info_span!("render.range", comp = %req.composition).entered();
        let mut count = 0;
        for index in req.range.frame_indices(req.fps) {
            let frame_req = RenderRequest {
                project: req.project,
                composition: req.composition,
                time: RationalTime::from_frames(index, req.fps),
                size: req.size,
                quality: req.quality,
                nodes: req.nodes,
            };
            let id = self.draw_frame(&frame_req)?;
            let frame = FrameRef::Gpu(GpuTexture { id, size: req.size });
            let sent = sink(index, frame, &mut self.gpu);
            // 区間のコマは受け取った側が読み戻して終わりなので、溜めずに捨てる。
            self.gpu.release_id(id);
            sent?;
            count += 1;
        }
        Ok(count)
    }

    fn render_audio(&mut self, req: &AudioRequest<'_>) -> Result<AudioBlock> {
        let _span = tracing::info_span!("render.audio", comp = %req.composition).entered();
        self.mix_audio(req)
    }

    fn invalidate(&mut self, _composition: &CompositionId, _range: TimeRange) {
        self.sources.clear();
    }

    fn gpu(&mut self) -> &mut dyn GpuHelper {
        &mut self.gpu
    }
}
