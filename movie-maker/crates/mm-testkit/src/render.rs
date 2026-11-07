//! レンダラーのモックと契約テスト。モックは CPU でレイヤーを下から順に source-over で重ねる。

use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use mm_audio_api::{AudioBlock, MixInput, MixerFactory};
use mm_decode_api::{DecoderFactory, FrameSource};
use mm_doc::{Composition, Layer, Project};
use mm_eval::{any_solo, audible, evaluate_audio, evaluate_opacity, node_params_at, source_time};
use mm_node_api::{
    audio_frames_in, AudioCx, FrameCx, FrameRef, GpuHelper, NoInputs, NodeKind, NodeRegistry,
    Quality, Services,
};
use mm_render_api::{mixer_config, AudioRequest, RangeRequest, RenderRequest, Renderer, Scheduler};
use mm_types::{
    AssetId, CompositionId, MmError, ParamValue, Params, RationalTime, Result, RgbaImage, Size,
    TimeRange,
};

use crate::decode::DecoderFrameSource;
use crate::gpu::CpuGpu;
use crate::render_cases;
use crate::xform::apply_transform;

/// モックのレンダラーのバージョン。
pub const SAMPLE_RENDERER_VERSION: u32 = 1;

/// モックのレンダラー。
pub struct SampleRenderer {
    decoders: Arc<dyn DecoderFactory>,
    mixers: Arc<dyn MixerFactory>,
    services: Services,
    gpu: CpuGpu,
    sources: HashMap<AssetId, Box<dyn FrameSource>>,
    errors: Vec<String>,
}

impl SampleRenderer {
    /// インターフェースを受け取って組む。
    pub fn new(
        decoders: Arc<dyn DecoderFactory>,
        mixers: Arc<dyn MixerFactory>,
        services: Services,
    ) -> Self {
        Self {
            decoders,
            mixers,
            services,
            gpu: CpuGpu::new(),
            sources: HashMap::new(),
            errors: Vec::new(),
        }
    }

    /// 透明にしたレイヤーのエラーを取り出す（フラグの元）。
    pub fn take_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.errors)
    }

    /// 評価済みのパラメータに `AssetRef` があれば、その素材を開いて（開いてあれば使い回して）返す。
    fn footage_for<'s>(
        sources: &'s mut HashMap<AssetId, Box<dyn FrameSource>>,
        decoders: &dyn DecoderFactory,
        project: &Project,
        params: &Params,
    ) -> Result<Option<&'s mut (dyn FrameSource + 's)>> {
        let Some(asset_id) = params.0.values().find_map(ParamValue::as_asset) else {
            return Ok(None);
        };
        if asset_id.as_str().is_empty() {
            return Ok(None);
        }
        if !sources.contains_key(asset_id) {
            let asset = project.asset(asset_id)?;
            let dec = decoders.open(std::path::Path::new(&asset.path), None)?;
            sources.insert(asset_id.clone(), Box::new(DecoderFrameSource::new(dec)));
        }
        Ok(mm_node_api::footage_ref(sources.get_mut(asset_id)))
    }

    fn evaluate(
        nodes: &dyn NodeRegistry,
        layer: &Layer,
        t: RationalTime,
    ) -> Result<(Arc<dyn NodeKind>, Params)> {
        let kind = nodes
            .get(&layer.source.kind)
            .ok_or_else(|| MmError::NotFound(format!("種類 {}", layer.source.kind)))?;
        let params = node_params_at(&layer.source, t).with_defaults(&kind.params());
        Ok((kind, params))
    }

    fn render_layer(
        &mut self,
        req: &RenderRequest<'_>,
        comp: &Composition,
        layer: &Layer,
    ) -> Result<Option<Arc<RgbaImage>>> {
        let (kind, params) = Self::evaluate(req.nodes, layer, req.time)?;
        let scale = req.size.width as f32 / comp.width as f32;
        let source_time = source_time(layer, req.time)?;
        let Self {
            sources,
            decoders,
            gpu,
            services,
            ..
        } = self;
        let mut inputs = NoInputs;
        let footage = Self::footage_for(sources, decoders.as_ref(), req.project, &params)?;
        let mut cx = FrameCx {
            time: req.time,
            params: &params,
            size: req.size,
            region: req.size.to_rect(),
            scale,
            quality: req.quality,
            inputs: &mut inputs,
            footage,
            source_time,
            gpu,
            services,
        };
        if kind.region_of_definition(&cx).is_empty() {
            return Ok(None);
        }
        let rendered = std::panic::catch_unwind(AssertUnwindSafe(|| kind.render(&mut cx)))
            .map_err(|_| MmError::Internal(format!("{} が panic", kind.id())))??;
        match rendered {
            FrameRef::Transparent { .. } => Ok(None),
            other => {
                let image = gpu.download(&other)?;
                Ok(Some(Arc::new(apply_transform(
                    &image,
                    &layer.transform,
                    req.time,
                    f64::from(scale),
                ))))
            }
        }
    }

    fn layer_audio(
        &mut self,
        req: &AudioRequest<'_>,
        layer: &Layer,
        overlap: TimeRange,
    ) -> Result<Option<MixInput>> {
        let (kind, params) = Self::evaluate(req.nodes, layer, overlap.start)?;
        if !kind.traits().audio {
            return Ok(None);
        }
        let source_range = TimeRange::new(
            source_time(layer, overlap.start)?,
            source_time(layer, overlap.end)?,
        )?;
        let footage = Self::footage_for(
            &mut self.sources,
            self.decoders.as_ref(),
            req.project,
            &params,
        )?;
        let mut cx = AudioCx {
            range: overlap,
            source_range,
            params: &params,
            sample_rate: req.sample_rate,
            channels: req.channels,
            footage,
        };
        let block = kind.render_audio(&mut cx)?;
        let offset = audio_frames_in(
            TimeRange::new(req.range.start, overlap.start)?,
            req.sample_rate,
        );
        let total = audio_frames_in(req.range, req.sample_rate);
        let (role, gain_db, pan) = layer_mix_settings(layer, overlap.start);
        Ok(Some(MixInput {
            block: place_block(&block, offset, total),
            role,
            gain_db,
            pan,
        }))
    }
}

/// レイヤーの音の設定（役割、dB、パン）を時刻 `t` で評価する。音の設定が無ければパススルー。
fn layer_mix_settings(layer: &Layer, t: RationalTime) -> (String, f32, f32) {
    let a = evaluate_audio(layer, t);
    (a.role.to_owned(), a.gain_db as f32, a.pan as f32)
}

/// `block` を `offset` フレーム目から置き、全体を `total` フレームにそろえる。
pub fn place_block(block: &AudioBlock, offset: usize, total: usize) -> AudioBlock {
    let ch = block.channels as usize;
    let mut samples = vec![0.0f32; total * ch];
    let n = block.frames().min(total.saturating_sub(offset));
    samples[offset * ch..(offset + n) * ch].copy_from_slice(&block.samples[..n * ch]);
    AudioBlock {
        sample_rate: block.sample_rate,
        channels: block.channels,
        samples,
    }
}

impl Renderer for SampleRenderer {
    fn id(&self) -> &'static str {
        crate::SAMPLE_ID
    }

    fn version(&self) -> u32 {
        SAMPLE_RENDERER_VERSION
    }

    fn render_frame(&mut self, req: &RenderRequest<'_>) -> Result<FrameRef> {
        let comp = req.project.composition(req.composition)?;
        let mut canvas = RgbaImage::transparent(req.size);
        for layer in &comp.layers {
            if !layer.range()?.contains(req.time) {
                continue;
            }
            let opacity = evaluate_opacity(layer, req.time) as f32;
            match self.render_layer(req, comp, layer) {
                Ok(Some(img)) => canvas.blit_over(&img, 0, 0, opacity),
                Ok(None) => {}
                Err(e) => self.errors.push(format!("layer {}: {e}", layer.id)),
            }
        }
        Ok(FrameRef::cpu(canvas))
    }

    fn render_audio(&mut self, req: &AudioRequest<'_>) -> Result<AudioBlock> {
        let comp = req.project.composition(req.composition)?;
        let mut inputs = Vec::new();
        let solo = any_solo(&comp.layers);
        for layer in comp.layers.iter().filter(|l| audible(l, solo)) {
            let Some(overlap) = layer.range()?.intersect(&req.range) else {
                continue;
            };
            if let Some(input) = self.layer_audio(req, layer, overlap)? {
                inputs.push(input);
            }
        }
        // 呼ぶたびに作り直すので、ダッキングの状態は区間の切れ目で戻る（モックのミキサーはダッキングを省く）。
        let config = mixer_config(comp.mixer.as_ref(), true);
        let mut mixer = self.mixers.create(req.sample_rate, req.channels, &config)?;
        mixer.mix(&inputs, audio_frames_in(req.range, req.sample_rate))
    }

    fn invalidate(&mut self, _composition: &CompositionId, _range: TimeRange) {
        self.sources.clear();
    }

    fn gpu(&mut self) -> &mut dyn GpuHelper {
        &mut self.gpu
    }
}

/// モックのスケジューラ。再生位置に近い範囲から。
#[derive(Debug, Default)]
pub struct NearestFirstScheduler;

impl Scheduler for NearestFirstScheduler {
    fn prioritize(&mut self, playhead: RationalTime, dirty: &[TimeRange]) -> Vec<TimeRange> {
        // 距離は並べ替えにだけ使うので浮動小数でよい。
        let distance = |r: &TimeRange| (r.start.to_f64() - playhead.to_f64()).abs();
        let mut v = dirty.to_vec();
        v.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
        v
    }
}

/// 契約テストに渡す描画の情報。
pub struct RenderFixture<'a> {
    pub project: &'a Project,
    pub composition: &'a CompositionId,
    pub nodes: &'a dyn NodeRegistry,
    pub size: Size,
}

/// レンダラーの契約。
pub fn contract(make: impl Fn() -> Box<dyn Renderer>, fx: &RenderFixture<'_>) {
    let comp = fx.project.composition(fx.composition).expect("ある");
    let mut r = make();
    let t = RationalTime::from_frames(1, comp.fps).expect("表せる");
    let req = RenderRequest {
        project: fx.project,
        composition: fx.composition,
        time: t,
        size: fx.size,
        quality: Quality::Full,
        nodes: fx.nodes,
    };
    let a = r.render_frame(&req).expect("描ける");
    assert_eq!(a.size(), fx.size, "頼んだ大きさ");
    let a = r.gpu().download(&a).expect("リードバック");
    let b = r.render_frame(&req).expect("描ける");
    let b = r.gpu().download(&b).expect("リードバック");
    assert_eq!(*a, *b, "同じ入力で bit 一致");

    let end = RationalTime::from_frames(3, comp.fps).expect("表せる");
    let range = TimeRange::new(RationalTime::ZERO, end).unwrap();
    let got = render_range_frames(r.as_mut(), fx, range, comp.fps);
    let indices: Vec<i64> = got.iter().map(|(i, _)| *i).collect();
    assert_eq!(indices, vec![0, 1, 2], "区間のフレームの番号");
    assert_eq!(
        *got[1].1, *a,
        "区間の 2 フレーム目は render_frame の同じ時刻と同じ"
    );

    r.invalidate(fx.composition, range);
    let c = r.render_frame(&req).expect("描ける");
    let c = r.gpu().download(&c).expect("リードバック");
    assert_eq!(*a, *c, "invalidate の後も同じ画像");

    check_audio(r.as_mut(), fx, range);
    render_cases::check_frame_lifetime(r.as_mut(), &req);
    render_cases::check_transform_and_failed_layer(make().as_mut(), fx);
}

fn render_range_frames(
    r: &mut dyn Renderer,
    fx: &RenderFixture<'_>,
    range: TimeRange,
    fps: mm_types::Rational,
) -> Vec<(i64, Arc<RgbaImage>)> {
    let mut got: Vec<(i64, Arc<RgbaImage>)> = Vec::new();
    let mut sink = |index: i64, frame: FrameRef, gpu: &mut dyn GpuHelper| -> Result<()> {
        got.push((index, gpu.download(&frame)?));
        Ok(())
    };
    let n = r
        .render_range(
            &RangeRequest {
                project: fx.project,
                composition: fx.composition,
                range,
                fps,
                size: fx.size,
                quality: Quality::Full,
                nodes: fx.nodes,
            },
            &mut sink,
        )
        .expect("区間が描ける");
    assert_eq!(n, got.len(), "返すのは渡したフレームの数");
    got
}

fn check_audio(r: &mut dyn Renderer, fx: &RenderFixture<'_>, range: TimeRange) {
    let audio = r
        .render_audio(&AudioRequest {
            project: fx.project,
            composition: fx.composition,
            range,
            sample_rate: mm_audio_api::DEFAULT_SAMPLE_RATE,
            channels: mm_audio_api::DEFAULT_CHANNELS,
            nodes: fx.nodes,
        })
        .expect("音が出る");
    assert_eq!(
        audio.frames(),
        audio_frames_in(range, mm_audio_api::DEFAULT_SAMPLE_RATE),
        "音のフレーム数は区間と合う"
    );
}
