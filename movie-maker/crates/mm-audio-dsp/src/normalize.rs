//! 書き出しのラウドネスの正規化。1 パス目で統合ラウドネスとトゥルーピークを測り、2 パス目で倍率をかけ、
//! トゥルーピークが上限を超える分だけ先読みのリミッターで削る。測りは `ebur128`（BS.1770-4、4 倍のオーバーサンプルのトゥルーピーク）。

use std::collections::VecDeque;

use ebur128::{EbuR128, Mode};

use mm_audio_api::{AudioBlock, LoudnessNormalizer, NormalizeReport, NormalizeSpec};
use mm_types::{MmError, Result};

use crate::mixer::db_to_gain;

/// リミッターの先読みと戻りの長さ（秒）。短いと歪み、長いと音が引っ込む。5 ms は放送のリミッターの一般的な値。
const LOOKAHEAD_SECONDS: f64 = 0.005;
/// 目標との差がこの値（LU）以内なら合わせ終わりとする。EBU R128 の許容 ±0.5 LU の 1/10。
const LUFS_TOLERANCE: f32 = 0.05;
/// 倍率とリミッターの調整を繰り返す上限。
const MAX_ROUNDS: usize = 8;
/// トゥルーピークが上限を超えたとき、超えた分に足して余裕を取る量（dB）。
const PEAK_MARGIN_DB: f32 = 0.02;

/// 測った値。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measurement {
    /// 統合ラウドネス（LUFS）。無音で測れなければ `None`。
    pub integrated_lufs: Option<f32>,
    /// トゥルーピーク（dBTP）。無音なら `-inf`。
    pub true_peak_dbtp: f32,
}

/// 統合ラウドネスとトゥルーピークを測る。
pub fn measure(block: &AudioBlock) -> Result<Measurement> {
    let mut state = EbuR128::new(
        u32::from(block.channels),
        block.sample_rate,
        Mode::I | Mode::TRUE_PEAK,
    )
    .map_err(|e| MmError::InvalidArgument(format!("メーターを作れない: {e}")))?;
    state
        .add_frames_f32(&block.samples)
        .map_err(|e| MmError::Internal(format!("ラウドネスの計測に失敗: {e}")))?;
    let lufs = state
        .loudness_global()
        .ok()
        .filter(|v| v.is_finite())
        .map(|v| v as f32);
    let mut peak = 0.0f64;
    for ch in 0..u32::from(block.channels) {
        peak = peak.max(state.true_peak(ch).unwrap_or(0.0));
    }
    Ok(Measurement {
        integrated_lufs: lufs,
        true_peak_dbtp: (20.0 * peak.log10()) as f32,
    })
}

/// 先読みのリミッター。標本のピークが `ceiling`（線形）を超えないよう、前後 `look` フレームにわたって滑らかに下げる。
/// 各フレームの必要な倍率の窓の最小値を、同じ長さの窓で平均するので、平均は必ずそのフレームの必要な倍率以下になる。
/// 削ったら `true`。
pub fn limit(block: &mut AudioBlock, ceiling: f32) -> bool {
    let channels = usize::from(block.channels);
    let frames = block.frames();
    let needed: Vec<f32> = block
        .samples
        .chunks_exact(channels)
        .map(|f| {
            let peak = f.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            if peak > ceiling {
                ceiling / peak
            } else {
                1.0
            }
        })
        .collect();
    if needed.iter().all(|g| *g >= 1.0) {
        return false;
    }
    let look = ((f64::from(block.sample_rate) * LOOKAHEAD_SECONDS) as usize).max(1);
    let gains = smooth(&window_min(&needed, look), look);
    for (frame, g) in block.samples.chunks_exact_mut(channels).zip(&gains) {
        frame.iter_mut().for_each(|s| *s *= g);
    }
    debug_assert_eq!(gains.len(), frames);
    true
}

/// 各位置の `[i - look, i + look]` の最小値。単調な deque で線形時間。
fn window_min(values: &[f32], look: usize) -> Vec<f32> {
    let n = values.len();
    let mut out = Vec::with_capacity(n);
    let mut deque: VecDeque<usize> = VecDeque::new();
    let mut next = 0;
    for i in 0..n {
        let hi = (i + look).min(n - 1);
        while next <= hi {
            while deque.back().is_some_and(|&b| values[b] >= values[next]) {
                deque.pop_back();
            }
            deque.push_back(next);
            next += 1;
        }
        while deque.front().is_some_and(|&f| f + look < i) {
            deque.pop_front();
        }
        out.push(deque.front().map_or(1.0, |&f| values[f]));
    }
    out
}

/// 各位置の `[i - look, i + look]` の平均。範囲の外は端の値で埋める（端のピークを薄めないため）。
fn smooth(values: &[f32], look: usize) -> Vec<f32> {
    if values.is_empty() {
        return Vec::new();
    }
    let n = values.len();
    let width = (2 * look + 1) as f64;
    let (first, last) = (f64::from(values[0]), f64::from(values[n - 1]));
    let mut prefix = Vec::with_capacity(n + 1);
    prefix.push(0.0f64);
    for v in values {
        prefix.push(prefix.last().copied().unwrap_or(0.0) + f64::from(*v));
    }
    (0..n)
        .map(|i| {
            let lo = i.saturating_sub(look);
            let hi = (i + look + 1).min(n);
            let left = (look - i.min(look)) as f64;
            let right = (i + look + 1 - hi) as f64;
            ((prefix[hi] - prefix[lo] + left * first + right * last) / width) as f32
        })
        .collect()
}

/// BS.1770 の統合ラウドネスに合わせる正規化。
#[derive(Debug, Clone, Copy, Default)]
pub struct DspLoudnessNormalizer;

impl LoudnessNormalizer for DspLoudnessNormalizer {
    fn normalize(&self, block: &mut AudioBlock, spec: &NormalizeSpec) -> Result<NormalizeReport> {
        let before = measure(block)?;
        let Some(measured) = before.integrated_lufs else {
            return Ok(NormalizeReport {
                measured_lufs: None,
                final_lufs: None,
                gain_db: 0.0,
                true_peak_dbtp: before.true_peak_dbtp,
                limited: false,
            });
        };
        let gain_db = spec.target_lufs - measured;
        let best = fit(block, spec, gain_db)?;
        *block = best.block;
        Ok(NormalizeReport {
            measured_lufs: Some(measured),
            final_lufs: best.after.integrated_lufs,
            gain_db,
            true_peak_dbtp: best.after.true_peak_dbtp,
            limited: best.limited,
        })
    }
}

/// 調整の途中の候補。
struct Candidate {
    block: AudioBlock,
    after: Measurement,
    limited: bool,
}

/// 倍率 `gain_db` から始め、トゥルーピークが上限に収まり統合ラウドネスが目標に合うまで調整する。
/// 上限は譲らない（収まった候補のうち最後のものを返す）。収まる候補が無ければ最後の候補を返す。
fn fit(source: &AudioBlock, spec: &NormalizeSpec, mut gain_db: f32) -> Result<Candidate> {
    let mut margin_db = 0.0f32;
    let mut kept: Option<Candidate> = None;
    for _ in 0..MAX_ROUNDS {
        let mut block = source.clone();
        let gain = db_to_gain(gain_db);
        block.samples.iter_mut().for_each(|s| *s *= gain);
        let limited = limit(&mut block, db_to_gain(spec.ceiling_dbtp - margin_db));
        let after = measure(&block)?;
        let over = after.true_peak_dbtp - spec.ceiling_dbtp;
        let err = spec.target_lufs - after.integrated_lufs.unwrap_or(spec.target_lufs);
        let candidate = Candidate {
            block,
            after,
            limited,
        };
        if over > 0.0 {
            margin_db += over + PEAK_MARGIN_DB;
            kept = kept.or(Some(candidate));
            continue;
        }
        let done = err.abs() <= LUFS_TOLERANCE;
        kept = Some(candidate);
        if done {
            break;
        }
        gain_db += err;
    }
    kept.ok_or_else(|| MmError::Internal("正規化の候補が無い".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;
    const SPEC_YT: NormalizeSpec = NormalizeSpec {
        target_lufs: -14.0,
        ceiling_dbtp: -1.0,
    };

    /// 1 kHz の正弦波（ステレオ）。`amp` は振幅（線形）。
    fn sine(seconds: f64, amp: f32) -> AudioBlock {
        let frames = (seconds * f64::from(RATE)) as usize;
        let mut b = AudioBlock::silence(RATE, 2, frames);
        for (i, f) in b.samples.chunks_exact_mut(2).enumerate() {
            let v = amp * (std::f32::consts::TAU * 1000.0 * i as f32 / RATE as f32).sin();
            f[0] = v;
            f[1] = v;
        }
        b
    }

    /// 持続する音に、短いパルスのピークを混ぜた音。目標のラウドネスに上げるとピークが上限を超える。
    fn peaky(seconds: f64) -> AudioBlock {
        let mut b = sine(seconds, 0.05);
        for f in b.samples.chunks_exact_mut(2).step_by(RATE as usize / 4) {
            f[0] = 0.9;
            f[1] = 0.9;
        }
        b
    }

    #[test]
    fn 小さい音は目標のラウドネスまで上がる() {
        let mut b = sine(5.0, 0.05);
        let r = DspLoudnessNormalizer.normalize(&mut b, &SPEC_YT).unwrap();
        let after = measure(&b).unwrap();
        assert!((after.integrated_lufs.unwrap() + 14.0).abs() < 0.1);
        assert!(after.true_peak_dbtp <= -1.0);
        assert!(!r.limited);
        assert!(r.gain_db > 0.0);
    }

    #[test]
    fn 大きい音は目標まで下がる() {
        let mut b = sine(5.0, 0.9);
        DspLoudnessNormalizer
            .normalize(
                &mut b,
                &NormalizeSpec {
                    target_lufs: -23.0,
                    ceiling_dbtp: -1.0,
                },
            )
            .unwrap();
        let after = measure(&b).unwrap();
        assert!((after.integrated_lufs.unwrap() + 23.0).abs() < 0.1);
    }

    #[test]
    fn ピークは上限に収まり_ラウドネスは目標に合う() {
        let mut b = peaky(6.0);
        let r = DspLoudnessNormalizer.normalize(&mut b, &SPEC_YT).unwrap();
        let after = measure(&b).unwrap();
        assert!(r.limited, "ピークを削る必要がある音のはず");
        assert!(after.true_peak_dbtp <= -1.0, "{}", after.true_peak_dbtp);
        assert!((after.integrated_lufs.unwrap() + 14.0).abs() < 0.5);
    }

    #[test]
    fn リミッターは上限を超える標本を残さない() {
        let mut b = peaky(2.0);
        b.samples.iter_mut().for_each(|s| *s *= 2.0);
        assert!(limit(&mut b, 0.5));
        let peak = b.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak <= 0.5 + 1e-6, "{peak}");
    }

    #[test]
    fn 上限に収まる音はリミッターが触らない() {
        let mut b = sine(1.0, 0.1);
        let orig = b.clone();
        assert!(!limit(&mut b, 0.5));
        assert_eq!(b, orig);
    }

    #[test]
    fn 無音は変えない() {
        let mut b = AudioBlock::silence(RATE, 2, RATE as usize);
        let r = DspLoudnessNormalizer.normalize(&mut b, &SPEC_YT).unwrap();
        assert_eq!(r.measured_lufs, None);
        assert!(b.samples.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn 窓の最小と平均は端でも長さを保つ() {
        let v = [1.0, 0.5, 1.0, 1.0, 1.0];
        let m = window_min(&v, 1);
        assert_eq!(m, vec![0.5, 0.5, 0.5, 1.0, 1.0]);
        assert_eq!(smooth(&m, 1).len(), v.len());
    }
}
