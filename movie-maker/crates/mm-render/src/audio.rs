//! 音。レイヤーごとにノードの種類の `render_audio` を呼び、置く位置をそろえてミキサーに渡す。
//! 入れ子（ノードの種類 `composition`）は、中のコンポジションを同じ手順で混ぜた音を 1 本の入力にする。

use mm_audio_api::{AudioBlock, MixInput, Mixer, MixerConfig};
use mm_doc::{Composition, Layer};
use mm_eval::{evaluate_audio, source_time};
use mm_node_api::{audio_frames_in, AudioCx};
use mm_render_api::AudioRequest;
use mm_types::{CompositionId, Id, Params, RationalTime, Result, TimeRange};

use crate::layers::{evaluate, MAX_DEPTH, NESTED_KIND, NESTED_PARAM};
use crate::renderer::WgpuRenderer;

/// 連続して呼ばれる `render_audio` の間で持ち越すミキサー。ダッキングの状態をブロックの切れ目で戻さない。
pub(crate) struct AudioStream {
    mixer: Box<dyn Mixer>,
    composition: CompositionId,
    /// 直前の区間の終わり。次の区間がここから始まれば続き、離れていればシーク。
    next_start: RationalTime,
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
        let inputs = self.comp_inputs(&scope)?;
        let mut stream = self.take_stream(req)?;
        let result = stream
            .mixer
            .mix(&inputs, audio_frames_in(req.range, req.sample_rate));
        stream.next_start = req.range.end;
        self.audio = Some(stream);
        result
    }

    /// 持ち越しのミキサーを取り出す。続きでなければ作り直す。
    fn take_stream(&mut self, req: &AudioRequest<'_>) -> Result<AudioStream> {
        let kept = self.audio.take().filter(|s| {
            s.composition == *req.composition
                && s.next_start == req.range.start
                && s.mixer.sample_rate() == req.sample_rate
                && s.mixer.channels() == req.channels
        });
        match kept {
            Some(stream) => Ok(stream),
            None => Ok(AudioStream {
                mixer: self.new_mixer(req.sample_rate, req.channels)?,
                composition: req.composition.clone(),
                next_start: req.range.start,
            }),
        }
    }

    fn new_mixer(&self, sample_rate: u32, channels: u16) -> Result<Box<dyn Mixer>> {
        self.mixers
            .create(sample_rate, channels, &MixerConfig::default())
    }

    /// コンポジションの区間に鳴るレイヤーの音を、区間の先頭に置いた入力として集める。
    fn comp_inputs(&mut self, scope: &AudioScope<'_>) -> Result<Vec<MixInput>> {
        let mut inputs = Vec::new();
        for layer in &scope.comp.layers {
            let Some(overlap) = layer.range()?.intersect(&scope.range) else {
                continue;
            };
            if let Some(input) = self.layer_audio(scope, layer, overlap)? {
                inputs.push(input);
            }
        }
        Ok(inputs)
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
        let block = if kind.id() == NESTED_KIND {
            match self.nested_audio(scope, &params, source_range)? {
                Some(block) => block,
                None => return Ok(None),
            }
        } else {
            let footage = self.sources.footage_for(
                self.decoders.as_ref(),
                self.gpu.context().as_ref(),
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
            kind.render_audio(&mut cx)?
        };
        let offset = audio_frames_in(
            TimeRange::new(scope.range.start, overlap.start)?,
            req.sample_rate,
        );
        let total = audio_frames_in(scope.range, req.sample_rate);
        let (role, gain_db, pan) = mix_settings(layer, overlap.start);
        Ok(Some(MixInput {
            block: place_block(&block, offset, total),
            role,
            gain_db,
            pan,
        }))
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
        if name.is_empty() {
            return Ok(None);
        }
        if scope.depth >= MAX_DEPTH {
            self.errors
                .push(format!("入れ子が {MAX_DEPTH} 段を超えた（循環の疑い）"));
            return Ok(None);
        }
        let id: CompositionId = Id::new(name);
        let comp = scope.req.project.composition(&id)?;
        let inner = AudioScope {
            comp,
            range: source_range,
            depth: scope.depth + 1,
            ..*scope
        };
        let inputs = self.comp_inputs(&inner)?;
        let frames = audio_frames_in(source_range, scope.req.sample_rate);
        let mut mixer = self.new_mixer(scope.req.sample_rate, scope.req.channels)?;
        mixer.mix(&inputs, frames).map(Some)
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
