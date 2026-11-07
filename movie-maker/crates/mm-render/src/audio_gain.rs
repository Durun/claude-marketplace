//! 時間で変わるレイヤーの音量。ミキサーは入力ごとに 1 つの音量しか受けず、書き出しは音を 10 秒ずつ混ぜるので、
//! 区間の先頭の 1 点で評価するとフェードと音量の動きが消える。キーフレームのある音量と音のフェードはレンダラーが
//! サンプルに掛け、ミキサーには 0 dB で渡す。

use mm_audio_api::AudioBlock;
use mm_doc::Layer;
use mm_eval::{eval_param, fade_gain, has_fades};
use mm_types::{RationalTime, Result};

/// 音量を評価する間隔（サンプル）。間は直線で補間する。48 kHz で約 1.3 ms。
const GAIN_STEP: usize = 64;

fn db_to_gain(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

/// `layer` の音量にキーフレームかフェードがあれば、`start`（コンポジションの時刻）から並ぶ `block` に掛けて `true` を返す。
pub(crate) fn apply_gain_envelope(
    block: &mut AudioBlock,
    layer: &Layer,
    start: RationalTime,
) -> Result<bool> {
    let Some(audio) = layer
        .audio
        .as_ref()
        .filter(|a| a.gain_db.is_animated() || has_fades(layer))
    else {
        return Ok(false);
    };
    let rate = block.sample_rate;
    let channels = usize::from(block.channels);
    let frames = block.frames();
    let gain_at = |i: usize| -> Result<f32> {
        let t = start.checked_add(RationalTime::new(i as i128, rate)?)?;
        let db = eval_param(&audio.gain_db, t).as_number().unwrap_or(0.0);
        Ok(db_to_gain(db) * fade_gain(layer, t) as f32)
    };
    let mut from = 0;
    let mut g0 = gain_at(0)?;
    while from < frames {
        let to = (from + GAIN_STEP).min(frames);
        let g1 = gain_at(to)?;
        let span = (to - from) as f32;
        for (k, frame) in block.samples[from * channels..to * channels]
            .chunks_exact_mut(channels)
            .enumerate()
        {
            let g = g0 + (g1 - g0) * k as f32 / span;
            frame.iter_mut().for_each(|s| *s *= g);
        }
        from = to;
        g0 = g1;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_doc::{Interpolation, Keyframe, LayerAudio, Node, Param};
    use mm_types::ParamValue;

    const RATE: u32 = 48_000;

    fn layer_with(gain_db: Param) -> Layer {
        let mut l = Layer::new(
            "a",
            Node::new("solid", 1),
            RationalTime::ZERO,
            RationalTime::ZERO,
            RationalTime::seconds(1),
        );
        l.audio = Some(LayerAudio {
            gain_db,
            ..LayerAudio::new("voice")
        });
        l
    }

    fn ones(frames: usize) -> AudioBlock {
        AudioBlock::from_samples(RATE, 2, vec![1.0; frames * 2]).unwrap()
    }

    #[test]
    fn 固定の音量は掛けずにミキサーへ任せる() {
        let l = layer_with(Param::constant(ParamValue::Number(-6.0)));
        let mut b = ones(100);
        assert!(!apply_gain_envelope(&mut b, &l, RationalTime::ZERO).unwrap());
        assert!(b.samples.iter().all(|s| *s == 1.0));
    }

    #[test]
    fn キーフレームの音量はブロックの中でも時刻ごとに掛かる() {
        let key = |t: i64, db: f64| {
            Keyframe::new(
                RationalTime::seconds(t),
                ParamValue::Number(db),
                Interpolation::Linear,
            )
        };
        let gain = Param {
            value: ParamValue::Number(0.0),
            keyframes: vec![key(0, 0.0), key(1, -20.0)],
        };
        let l = layer_with(gain);
        let mut b = ones(RATE as usize);
        assert!(apply_gain_envelope(&mut b, &l, RationalTime::ZERO).unwrap());
        let at = |i: usize| b.samples[i * 2];
        assert!((at(0) - 1.0).abs() < 1e-4, "先頭は 0 dB");
        let half = 10f32.powf(-10.0 / 20.0);
        assert!(
            (at(RATE as usize / 2) - half).abs() < 1e-3,
            "中ほどは -10 dB"
        );
        assert!(at(RATE as usize - 1) < 0.11, "終わりは -20 dB 近く");
    }

    #[test]
    fn 固定の音量でもフェードがあれば時刻ごとに掛かる() {
        let mut l = layer_with(Param::constant(ParamValue::Number(-6.0)));
        let audio = l.audio.as_mut().unwrap();
        audio.fade_out = Some(mm_doc::Fade::linear(RationalTime::new(1, 2).unwrap()));
        let mut b = ones(RATE as usize);
        assert!(apply_gain_envelope(&mut b, &l, RationalTime::ZERO).unwrap());
        let at = |i: usize| b.samples[i * 2];
        let base = 10f32.powf(-6.0 / 20.0);
        assert!(
            (at(RATE as usize / 4) - base).abs() < 1e-4,
            "フェードの前は -6 dB"
        );
        assert!(
            (at(RATE as usize * 3 / 4) - base * 0.5).abs() < 1e-3,
            "末尾の 0.25 秒前は振幅が半分"
        );
    }

    #[test]
    fn 区切りの位置によらず同じ音量が掛かる() {
        // プレビューは 0.1 秒、書き出しは 10 秒ずつ混ぜるので、区切りが変わっても掛かる音量は同じでなければならない
        let key = |t: i64, db: f64| {
            Keyframe::new(
                RationalTime::seconds(t),
                ParamValue::Number(db),
                Interpolation::Linear,
            )
        };
        let l = layer_with(Param {
            value: ParamValue::Number(0.0),
            keyframes: vec![key(0, -40.0), key(1, 0.0)],
        });
        let mut whole = ones(RATE as usize);
        apply_gain_envelope(&mut whole, &l, RationalTime::ZERO).unwrap();
        let cut = 1_001;
        let mut head = ones(cut);
        let mut tail = ones(RATE as usize - cut);
        apply_gain_envelope(&mut head, &l, RationalTime::ZERO).unwrap();
        let tail_start = RationalTime::new(cut as i128, RATE).unwrap();
        apply_gain_envelope(&mut tail, &l, tail_start).unwrap();
        let split: Vec<f32> = head.samples.into_iter().chain(tail.samples).collect();
        let worst = whole
            .samples
            .iter()
            .zip(&split)
            .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(worst < 1e-3, "最大の差 {worst}");
    }
}
