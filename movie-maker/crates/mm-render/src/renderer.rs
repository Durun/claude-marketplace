//! wgpu のレンダラー。`Renderer` の実装。

use std::collections::VecDeque;
use std::sync::Arc;

use mm_audio_api::{AudioBlock, MixerFactory};
use mm_decode_api::{DecoderFactory, Encoder};
use mm_node_api::{FrameRef, GpuHelper, GpuTexture, Services};
use mm_render_api::{AudioRequest, RangeRequest, RenderRequest, Renderer, TrackLevel};
use mm_types::{CompositionId, RationalTime, Result, TimeRange};

use crate::audio::AudioStream;
use crate::compose::Compositor;
use crate::export::ExportPipe;
use crate::frame_cache::{FrameCacheConfig, MemoStats, RenderCache};
use crate::layers::CompCx;
use crate::source::SourcePool;
use mm_gpu::{GpuContext, PassTimer, WgpuGpu};

/// レンダラーのバージョン。合成の式を変えたら上げる（フレームのキャッシュのキーに入る）。
pub const WGPU_RENDERER_VERSION: u32 = 1;
/// `render_frame` が返したテクスチャを持っておく数。これより古いフレームの `FrameRef` は無効になる。
const KEPT_OUTPUTS: usize = mm_render_api::KEPT_RENDERED_FRAMES;

/// wgpu のレンダラー。
pub struct WgpuRenderer {
    pub(crate) gpu: WgpuGpu,
    pub(crate) compositor: Compositor,
    /// `gpu` と同じタイマー。合成のパスが書く。
    pub(crate) timer: Arc<PassTimer>,
    pub(crate) decoders: Arc<dyn DecoderFactory>,
    pub(crate) mixers: Arc<dyn MixerFactory>,
    pub(crate) services: Services,
    pub(crate) sources: SourcePool,
    pub(crate) cache: RenderCache,
    pub(crate) errors: Vec<String>,
    pub(crate) audio: Option<AudioStream>,
    /// 直前の `render_audio` のトラックのメーターの値。
    pub(crate) audio_levels: Vec<TrackLevel>,
    outputs: VecDeque<u64>,
    /// デバイスが消えて作り直した回数。
    recoveries: u32,
    /// 書き出しの NV12 への変換。最初の書き出しで作る。
    pub(crate) export: Option<ExportPipe>,
}

impl WgpuRenderer {
    /// コンテキストとインターフェースの実装を受け取って組む。
    pub fn new(
        ctx: Arc<GpuContext>,
        decoders: Arc<dyn DecoderFactory>,
        mixers: Arc<dyn MixerFactory>,
        services: Services,
    ) -> Self {
        let gpu = WgpuGpu::new(ctx.clone());
        Self {
            compositor: Compositor::new(&ctx),
            timer: gpu.timer().clone(),
            gpu,
            decoders,
            mixers,
            services,
            sources: SourcePool::default(),
            cache: RenderCache::new(FrameCacheConfig::default()),
            errors: Vec::new(),
            audio: None,
            audio_levels: Vec::new(),
            outputs: VecDeque::new(),
            recoveries: 0,
            export: None,
        }
    }

    /// フレームのキャッシュの設定を差し替える（メモリの上限、退避先）。覚えていたものは捨てる。
    pub fn with_frame_cache(mut self, config: FrameCacheConfig) -> Self {
        self.cache.clear();
        self.cache.flush(&mut self.gpu);
        self.cache = RenderCache::new(config);
        self
    }

    /// フレームのキャッシュの数え上げ（引いた回数、当たった回数、大きさ）。
    pub fn frame_cache_stats(&self) -> MemoStats {
        self.cache.stats()
    }

    /// 追い出した画像のディスクへの退避（別のスレッドで書く）が、ここまでの分だけ終わるまで待つ。
    pub fn wait_frame_cache_spills(&mut self) {
        self.cache.wait_spills();
    }

    /// 透明にしたレイヤーのエラーを取り出す（フラグの元）。
    pub fn take_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.errors)
    }

    /// 使っている GPU のコンテキスト。作り直した後は新しいもの。
    pub fn context(&self) -> &Arc<GpuContext> {
        self.gpu.context()
    }

    /// デバイスが消えて作り直した回数。
    pub fn recoveries(&self) -> u32 {
        self.recoveries
    }

    /// デバイスが消えていたら、同じ求めでコンテキストを作り直し、その上の全部（合成のパイプライン、
    /// タイマー、テクスチャ、開いた素材）を作り直す。前のフレームの `FrameRef` は無効になる。
    fn ensure_device(&mut self) -> Result<()> {
        if !self.gpu.context().is_lost() {
            return Ok(());
        }
        let _span = tracing::info_span!("render.recover_device").entered();
        let ctx = self.gpu.context().recreate()?;
        self.rebind(ctx);
        self.recoveries += 1;
        mm_trace::counter!("render.device_recoveries", 1);
        tracing::warn!(
            adapter = self.gpu.context().adapter_name(),
            "GPU のデバイスを作り直した"
        );
        Ok(())
    }

    /// GPU の上の持ち物を `ctx` の上に作り直す。
    fn rebind(&mut self, ctx: Arc<GpuContext>) {
        self.compositor = Compositor::new(&ctx);
        self.gpu = WgpuGpu::new(ctx);
        self.timer = self.gpu.timer().clone();
        self.outputs.clear();
        // 覚えた画像は前のデバイスのテクスチャなので、解放せず忘れる。
        self.cache.forget_all();
        // 開いた素材は前のデバイスの上でデコードしているので、次に使うとき開き直す。
        self.sources.clear();
        self.export = None;
    }

    /// 持っているテクスチャの数。漏れの検査に使う。
    pub fn texture_count(&self) -> usize {
        self.gpu.texture_count()
    }

    /// 読み取りが終わった GPU の時間を `mm-trace` に載せる。待たないので、結果は 1〜2 フレーム遅れる。
    ///
    /// GPU の時間はスパンの開閉で測れないため、測った長さをそのままスパン `render.gpu_frame` の
    /// 分布に載せ、直近の値をゲージ `render.gpu_frame_us` にも出す。
    fn report_gpu_time(&mut self) {
        Self::report(self.timer.collect());
    }

    /// 頼んだ GPU の時間の読み取りが終わるまで待ち、全部を `mm-trace` に載せる。
    /// 通常の描画は待たないので、最後の数フレームの分は次の描画まで載らない。結果を確定させたい所で呼ぶ。
    pub fn flush_gpu_time(&mut self) {
        Self::report(self.timer.collect_all());
    }

    fn report(times_ns: Vec<u64>) {
        for ns in times_ns {
            mm_trace::record_span("render.gpu_frame", std::time::Duration::from_nanos(ns));
            mm_trace::gauge!("render.gpu_frame_us", ns / 1_000);
        }
    }

    fn keep_output(&mut self, id: u64) {
        self.outputs.push_back(id);
        while self.outputs.len() > KEPT_OUTPUTS {
            if let Some(old) = self.outputs.pop_front() {
                self.gpu.release_id(old);
            }
        }
    }

    pub(crate) fn draw_frame(&mut self, req: &RenderRequest<'_>) -> Result<u64> {
        self.ensure_device()?;
        self.cache.flush(&mut self.gpu);
        self.sources.begin_frame();
        self.report_gpu_time();
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
        // 借用した GPU のフレームは、この提出が終わるまでの借用（RFC 0001）。前に待ち、後に返す。
        self.gpu.before_submit();
        ctx.queue.submit([encoder.finish()]);
        self.gpu.after_submit();
        self.timer.after_submit();
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
                time: RationalTime::from_frames(index, req.fps)?,
                size: req.size,
                quality: req.quality,
                nodes: req.nodes,
            };
            let id = self.draw_frame(&frame_req)?;
            let frame = FrameRef::Gpu(GpuTexture { id, size: req.size });
            let sent = sink(index, frame, &mut self.gpu);
            // 区間のフレームは受け取った側がリードバックして終わりなので、溜めずにドロップする。
            self.gpu.release_id(id);
            sent?;
            count += 1;
        }
        Ok(count)
    }

    fn export_range(
        &mut self,
        req: &RangeRequest<'_>,
        encoder: &mut dyn Encoder,
        _batch: usize,
        sink: &mut dyn FnMut(i64),
    ) -> Result<usize> {
        self.export_nv12(req, encoder, sink)
    }

    fn render_audio(&mut self, req: &AudioRequest<'_>) -> Result<AudioBlock> {
        let _span = tracing::info_span!("render.audio", comp = %req.composition).entered();
        self.mix_audio(req)
    }

    fn audio_levels(&self) -> Vec<TrackLevel> {
        self.audio_levels.clone()
    }

    fn invalidate(&mut self, composition: &CompositionId, _range: TimeRange) {
        // 範囲は見ない。入れ子の画像は時刻をキーに持つので、残っても別の時刻の画像は引かれない。
        self.cache.invalidate(&[composition.as_str().to_owned()]);
        // 編集で音が変わるので、ダッキングなどの持ち越しも破棄する。
        self.audio = None;
    }

    fn invalidate_touched(&mut self, ids: &[String]) {
        self.cache.invalidate(ids);
        self.sources.forget(ids);
        self.audio = None;
    }

    fn gpu(&mut self) -> &mut dyn GpuHelper {
        &mut self.gpu
    }
}
