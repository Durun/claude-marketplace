//! 音。レイヤーごとに種類の `render_audio` を呼び、置く位置をそろえてミキサーに渡す。

use mm_audio_api::{AudioBlock, MixInput};
use mm_doc::{Layer, Param};
use mm_node_api::{audio_frames_in, AudioCx};
use mm_render_api::AudioRequest;
use mm_types::{RationalTime, Result, TimeRange};

use crate::layers::evaluate;
use crate::renderer::WgpuRenderer;

impl WgpuRenderer {
    /// 区間の音を混ぜる。
    pub(crate) fn mix_audio(&mut self, req: &AudioRequest<'_>) -> Result<AudioBlock> {
        let comp = req.project.composition(req.composition)?;
        let mut inputs = Vec::new();
        for layer in &comp.layers {
            let Some(overlap) = layer.range()?.intersect(&req.range) else {
                continue;
            };
            if let Some(input) = self.layer_audio(req, layer, overlap)? {
                inputs.push(input);
            }
        }
        let mut mixer = self.mixers.create(req.sample_rate, req.channels)?;
        mixer.mix(&inputs, audio_frames_in(req.range, req.sample_rate))
    }

    fn layer_audio(
        &mut self,
        req: &AudioRequest<'_>,
        layer: &Layer,
        overlap: TimeRange,
    ) -> Result<Option<MixInput>> {
        let (kind, params) = evaluate(req.nodes, layer, overlap.start)?;
        if !kind.traits().audio {
            return Ok(None);
        }
        let source_range = TimeRange::new(
            layer.to_source_time(overlap.start),
            layer.to_source_time(overlap.end),
        )?;
        let footage = self
            .sources
            .footage_for(self.decoders.as_ref(), req.project, &params)?;
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
        let (role, gain_db, pan) = mix_settings(layer, overlap.start);
        Ok(Some(MixInput {
            block: place_block(&block, offset, total),
            role,
            gain_db,
            pan,
        }))
    }
}

/// レイヤーの音の設定（役割、dB、パン）を時刻 `t` で評価する。設定が無ければ素通し。
fn mix_settings(layer: &Layer, t: RationalTime) -> (String, f32, f32) {
    let Some(a) = &layer.audio else {
        return ("voice".into(), 0.0, 0.0);
    };
    let number = |p: &Param| p.hold_value_at(t).as_number().unwrap_or(0.0) as f32;
    (a.role.clone(), number(&a.gain_db), number(&a.pan))
}

/// `block` を `offset` コマ目から置き、全体を `total` コマにそろえる。
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
