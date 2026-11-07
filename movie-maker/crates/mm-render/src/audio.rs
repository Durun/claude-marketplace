//! 音。レイヤーごとにノードの種類の `render_audio` を呼び、置く位置をそろえてミキサーに渡す。
//! 入れ子（ノードの種類 `composition`）と sequence は、中のコンポジションやカットの列を同じ手順で混ぜた音を 1 本の入力にする。

use mm_audio_api::{AudioBlock, MixInput, Mixer, MixerConfig};
use mm_doc::{Composition, Layer, SEQUENCE_KIND, SEQUENCE_PARAM};
use mm_eval::{any_solo, audible, evaluate_audio, place_cuts, source_time};
use mm_node_api::{audio_frames_in, AudioCx, NodeKind};
use mm_render_api::{mixer_config, AudioRequest, TrackLevel};
use mm_types::{CompositionId, Id, LayerId, Params, RationalTime, Result, TimeRange};

use crate::audio_gain::apply_gain_envelope;
use crate::layers::{evaluate, MAX_DEPTH, NESTED_KIND, NESTED_PARAM};
use crate::renderer::WgpuRenderer;
use crate::sequence::cut_layer;

/// 連続して呼ばれる `render_audio` の間で持ち越すミキサー。ダッキングの状態をブロックの切れ目で戻さない。
pub(crate) struct AudioStream {
    mixer: Box<dyn Mixer>,
    composition: CompositionId,
    /// ミキサーを作った設定。Mixer で設定が変わったら作り直す。
    config: MixerConfig,
    /// 直前の区間の終わり。次の区間がここから始まれば続き、離れていればシーク。
    next_start: RationalTime,
}

/// ミキサーに渡す入力と、それぞれを出したレイヤー（メーターの値をレイヤーに戻すため）。
struct CompInputs {
    layers: Vec<LayerId>,
    inputs: Vec<MixInput>,
}

/// 1 回の `render_audio` の入力に加えて、混ぜる区間とコンポジションの深さ。
#[derive(Clone, Copy)]
struct AudioScope<'a> {
    req: &'a AudioRequest<'a>,
    comp: &'a Composition,
    /// `comp` の時間軸での区間。
    range: TimeRange,
    depth: u32,
}

impl WgpuRenderer {
    /// 区間の音を混ぜる。前の区間の続きなら同じミキサーを使い、シークなら作り直す。
    pub(crate) fn mix_audio(&mut self, req: &AudioRequest<'_>) -> Result<AudioBlock> {
        let comp = req.project.composition(req.composition)?;
        let scope = AudioScope {
            req,
            comp,
            range: req.range,
            depth: 0,
        };
        let comp_inputs = self.comp_inputs(&scope)?;
        let config = mixer_config(comp.mixer.as_ref(), true);
        let mut stream = self.take_stream(req, config)?;
        let result = stream.mixer.mix(
            &comp_inputs.inputs,
            audio_frames_in(req.range, req.sample_rate),
        );
        // 混ぜられなかった区間のピークは途中までの値なので、メーターに出さない。
        self.audio_levels = match &result {
            Ok(_) => comp_inputs
                .layers
                .into_iter()
                .zip(stream.mixer.input_peaks().iter().copied())
                .map(|(layer, peak)| TrackLevel { layer, peak })
                .collect(),
            Err(_) => Vec::new(),
        };
        stream.next_start = req.range.end;
        self.audio = Some(stream);
        result
    }

    /// 持ち越しのミキサーを取り出す。続きでないか、設定が変わっていれば作り直す。
    fn take_stream(&mut self, req: &AudioRequest<'_>, config: MixerConfig) -> Result<AudioStream> {
        let kept = self.audio.take().filter(|s| {
            s.composition == *req.composition
                && s.next_start == req.range.start
                && s.config == config
                && s.mixer.sample_rate() == req.sample_rate
                && s.mixer.channels() == req.channels
        });
        match kept {
            Some(stream) => Ok(stream),
            None => Ok(AudioStream {
                mixer: self.new_mixer(req.sample_rate, req.channels, &config)?,
                composition: req.composition.clone(),
                config,
                next_start: req.range.start,
            }),
        }
    }

    fn new_mixer(
        &self,
        sample_rate: u32,
        channels: u16,
        config: &MixerConfig,
    ) -> Result<Box<dyn Mixer>> {
        self.mixers.create(sample_rate, channels, config)
    }

    /// コンポジションの区間に鳴るレイヤーの音を、区間の先頭に置いた入力として集める。
    /// ミュートのレイヤーと、ソロがあるときのソロでないレイヤーは混ぜない。
    fn comp_inputs(&mut self, scope: &AudioScope<'_>) -> Result<CompInputs> {
        let mut out = CompInputs {
            layers: Vec::new(),
            inputs: Vec::new(),
        };
        let solo = any_solo(&scope.comp.layers);
        for layer in scope.comp.layers.iter().filter(|l| audible(l, solo)) {
            let Some(overlap) = layer.range()?.intersect(&scope.range) else {
                continue;
            };
            if let Some(input) = self.layer_audio(scope, layer, overlap)? {
                out.layers.push(layer.id.clone());
                out.inputs.push(input);
            }
        }
        Ok(out)
    }

    fn layer_audio(
        &mut self,
        scope: &AudioScope<'_>,
        layer: &Layer,
        overlap: TimeRange,
    ) -> Result<Option<MixInput>> {
        let req = scope.req;
        let (kind, params) = evaluate(req.nodes, layer, overlap.start)?;
        if !kind.traits().audio {
            return Ok(None);
        }
        let source_range = TimeRange::new(
            source_time(layer, overlap.start)?,
            source_time(layer, overlap.end)?,
        )?;
        let block = match kind.id() {
            NESTED_KIND => self.nested_audio(scope, &params, source_range)?,
            SEQUENCE_KIND => self.sequence_audio(scope, &params, source_range)?,
            _ => Some(self.node_audio(req, kind.as_ref(), &params, overlap, source_range)?),
        };
        let Some(mut block) = block else {
            return Ok(None);
        };
        let offset = audio_frames_in(
            TimeRange::new(scope.range.start, overlap.start)?,
            req.sample_rate,
        );
        let total = audio_frames_in(scope.range, req.sample_rate);
        let (role, mut gain_db, pan) = mix_settings(layer, overlap.start);
        if apply_gain_envelope(&mut block, layer, overlap.start)? {
            gain_db = 0.0;
        }
        Ok(Some(MixInput {
            block: place_block(&block, offset, total),
            role,
            gain_db,
            pan,
        }))
    }

    /// 入れ子でも sequence でもないノードの音。素材があればその区間を読む。
    fn node_audio(
        &mut self,
        req: &AudioRequest<'_>,
        kind: &dyn NodeKind,
        params: &Params,
        range: TimeRange,
        source_range: TimeRange,
    ) -> Result<AudioBlock> {
        let footage = self.sources.footage_for(
            self.decoders.as_ref(),
            self.gpu.context().as_ref(),
            req.project,
            params,
            source_range.start,
        )?;
        let mut cx = AudioCx {
            range,
            source_range,
            params,
            sample_rate: req.sample_rate,
            channels: req.channels,
            footage,
        };
        kind.render_audio(&mut cx)
    }

    /// sequence の `source_range`（sequence の時間軸）の音。有効なカットを、映像と同じくカットの位置・イン点・速さを
    /// 持つレイヤーにして、入れ子と同じ手順で混ぜる。境のトランジションは音に掛けず、境で切り替える。
    fn sequence_audio(
        &mut self,
        scope: &AudioScope<'_>,
        params: &Params,
        source_range: TimeRange,
    ) -> Result<Option<AudioBlock>> {
        let name = params
            .get(SEQUENCE_PARAM)
            .and_then(|v| v.as_text())
            .unwrap_or("");
        if name.is_empty() || !self.within_depth(scope) {
            return Ok(None);
        }
        let seq = scope.req.project.sequence(&Id::new(name))?;
        let layers = place_cuts(seq)?
            .iter()
            .map(|p| cut_layer(&seq.cuts[p.index], p))
            .collect();
        let cuts = Composition::new(
            Id::new(name),
            scope.comp.width,
            scope.comp.height,
            scope.comp.fps,
            scope.comp.duration,
        )
        .with_layers(layers);
        self.mix_range(scope, &cuts, source_range).map(Some)
    }

    /// 入れ子の深さが上限の中か。超えたらエラーを記録する。
    fn within_depth(&mut self, scope: &AudioScope<'_>) -> bool {
        let ok = scope.depth < MAX_DEPTH;
        if !ok {
            self.errors
                .push(format!("入れ子が {MAX_DEPTH} 段を超えた（循環の疑い）"));
        }
        ok
    }

    /// コンポジション `comp` の `range` を、そのバスとマスターの設定を持ちダッキングの無い別のミキサーで 1 本に混ぜる。
    fn mix_range(
        &mut self,
        scope: &AudioScope<'_>,
        comp: &Composition,
        range: TimeRange,
    ) -> Result<AudioBlock> {
        let inner = AudioScope {
            comp,
            range,
            depth: scope.depth + 1,
            ..*scope
        };
        let inputs = self.comp_inputs(&inner)?.inputs;
        let frames = audio_frames_in(range, scope.req.sample_rate);
        let config = mixer_config(comp.mixer.as_ref(), false);
        let mut mixer = self.new_mixer(scope.req.sample_rate, scope.req.channels, &config)?;
        mixer.mix(&inputs, frames)
    }

    /// 入れ子のコンポジションの `source_range` を、ダッキングなしの別のミキサーで 1 本に混ぜる。
    /// 役割・音量・パンは入れ子のレイヤーの設定で親が扱う。
    fn nested_audio(
        &mut self,
        scope: &AudioScope<'_>,
        params: &Params,
        source_range: TimeRange,
    ) -> Result<Option<AudioBlock>> {
        let name = params
            .get(NESTED_PARAM)
            .and_then(|v| v.as_text())
            .unwrap_or("");
        if name.is_empty() || !self.within_depth(scope) {
            return Ok(None);
        }
        let id: CompositionId = Id::new(name);
        let comp = scope.req.project.composition(&id)?;
        self.mix_range(scope, comp, source_range).map(Some)
    }
}

/// レイヤーの音の設定（役割、dB、パン）を時刻 `t` で評価する。設定が無ければパススルー。
fn mix_settings(layer: &Layer, t: RationalTime) -> (String, f32, f32) {
    let a = evaluate_audio(layer, t);
    (a.role.to_owned(), a.gain_db as f32, a.pan as f32)
}

/// `block` を `offset` フレーム目から置き、全体を `total` フレームにそろえる。
fn place_block(block: &AudioBlock, offset: usize, total: usize) -> AudioBlock {
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
