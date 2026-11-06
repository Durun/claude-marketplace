//! 書き出しのラウドネスの正規化。1 パス目で統合ラウドネスとトゥルーピークを測り、2 パス目で倍率をかけ、
//! トゥルーピークが上限を超える分だけ先読みのリミッターで削る。測りは `ebur128`（BS.1770-4、4 倍のオーバーサンプルのトゥルーピーク）。

use std::collections::VecDeque;

use ebur128::{EbuR128, Mode};

use mm_audio_api::{AudioBlock, LoudnessNormalizer, NormalizeReport, NormalizeSpec};
use mm_types::{MmError, Result};

use crate::mixer::db_to_gain;

/// 測りのとき一度に一時バッファへ作るフレーム数。大きいほど呼び出しが減り、小さいほどメモリが減る。
const MEASURE_CHUNK_FRAMES: usize = 1 << 16;
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
    let mut state = new_meter(block.sample_rate, block.channels)?;
    feed(&mut state, &block.samples)?;
    read_meter(&state, block.channels)
}

fn new_meter(sample_rate: u32, channels: u16) -> Result<EbuR128> {
    EbuR128::new(u32::from(channels), sample_rate, Mode::I | Mode::TRUE_PEAK)
        .map_err(|e| MmError::InvalidArgument(format!("メーターを作れない: {e}")))
}

fn feed(state: &mut EbuR128, samples: &[f32]) -> Result<()> {
    state
        .add_frames_f32(samples)
        .map_err(|e| MmError::Internal(format!("ラウドネスの計測に失敗: {e}")))
}

fn read_meter(state: &EbuR128, channels: u16) -> Result<Measurement> {
    let lufs = state
        .loudness_global()
        .ok()
        .filter(|v| v.is_finite())
        .map(|v| v as f32);
    let mut peak = 0.0f64;
    for ch in 0..u32::from(channels) {
        peak = peak.max(state.true_peak(ch).unwrap_or(0.0));
    }
    Ok(Measurement {
        integrated_lufs: lufs,
        true_peak_dbtp: (20.0 * peak.log10()) as f32,
    })
}

/// 元の標本に `gain` をかけ、さらにフレームごとに `limiter` の倍率をかける、という変換の記述。標本は複製しない。
struct Envelope {
    gain: f32,
    /// フレームごとのリミッターの倍率。削る所が無ければ `None`。
    limiter: Option<Vec<f32>>,
}

impl Envelope {
    /// `src`（先頭が `start_frame` フレーム目）に変換をかけて `dst` へ書く。
    fn apply(&self, src: &[f32], channels: usize, start_frame: usize, dst: &mut Vec<f32>) {
        dst.clear();
        dst.extend(src.iter().map(|s| *s * self.gain));
        let Some(limiter) = &self.limiter else {
            return;
        };
        for (frame, g) in dst.chunks_exact_mut(channels).zip(&limiter[start_frame..]) {
            frame.iter_mut().for_each(|s| *s *= g);
        }
    }

    /// 変換した後の音を測る。一時バッファは `MEASURE_CHUNK_FRAMES` フレーム分だけ。
    fn measure(&self, source: &AudioBlock) -> Result<Measurement> {
        let channels = usize::from(source.channels);
        let mut state = new_meter(source.sample_rate, source.channels)?;
        let mut buf = Vec::with_capacity(MEASURE_CHUNK_FRAMES * channels);
        for (i, chunk) in source
            .samples
            .chunks(MEASURE_CHUNK_FRAMES * channels)
            .enumerate()
        {
            self.apply(chunk, channels, i * MEASURE_CHUNK_FRAMES, &mut buf);
            feed(&mut state, &buf)?;
        }
        read_meter(&state, source.channels)
    }

    /// 元の標本へ 1 回だけかける。
    fn apply_in_place(&self, block: &mut AudioBlock) {
        let channels = usize::from(block.channels);
        block.samples.iter_mut().for_each(|s| *s *= self.gain);
        let Some(limiter) = &self.limiter else {
            return;
        };
        for (frame, g) in block.samples.chunks_exact_mut(channels).zip(limiter) {
            frame.iter_mut().for_each(|s| *s *= g);
        }
    }
}

/// フレームごとのピークの列から、リミッターの倍率の列を作る。削る所が無ければ `None`。
fn limiter_gains(
    peaks: impl Iterator<Item = f32>,
    sample_rate: u32,
    ceiling: f32,
) -> Option<Vec<f32>> {
    let needed: Vec<f32> = peaks
        .map(|peak| if peak > ceiling { ceiling / peak } else { 1.0 })
        .collect();
    if needed.iter().all(|g| *g >= 1.0) {
        return None;
    }
    let look = ((f64::from(sample_rate) * LOOKAHEAD_SECONDS) as usize).max(1);
    Some(smooth(&window_min(&needed, look), look))
}

fn frame_peaks(samples: &[f32], channels: usize) -> Vec<f32> {
    samples
        .chunks_exact(channels)
        .map(|f| f.iter().fold(0.0f32, |m, s| m.max(s.abs())))
        .collect()
}

/// 先読みのリミッター。標本のピークが `ceiling`（線形）を超えないよう、前後 `look` フレームにわたって滑らかに下げる。
/// 各フレームの必要な倍率の窓の最小値を、同じ長さの窓で平均するので、平均は必ずそのフレームの必要な倍率以下になる。
/// 削ったら `true`。
pub fn limit(block: &mut AudioBlock, ceiling: f32) -> bool {
    let peaks = frame_peaks(&block.samples, usize::from(block.channels));
    let Some(limiter) = limiter_gains(peaks.into_iter(), block.sample_rate, ceiling) else {
        return false;
    };
    Envelope {
        gain: 1.0,
        limiter: Some(limiter),
    }
    .apply_in_place(block);
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
        best.envelope.apply_in_place(block);
        Ok(NormalizeReport {
            measured_lufs: Some(measured),
            final_lufs: best.after.integrated_lufs,
            gain_db,
            true_peak_dbtp: best.after.true_peak_dbtp,
            limited: best.limited,
        })
    }
}

/// 調整の途中の候補。標本は持たず、変換の記述だけ持つ。
struct Candidate {
    envelope: Envelope,
    after: Measurement,
    limited: bool,
    /// トゥルーピークが上限を超えた量（dB）。収まっていれば 0 以下。
    over: f32,
}

/// 候補の選び方。上限に収まった候補は後のものほど優先し、どれも収まらなければ超えた量が最小のものを残す。
#[derive(Default)]
struct Best(Option<Candidate>);

impl Best {
    fn offer(&mut self, candidate: Candidate) {
        let replace = match &self.0 {
            None => true,
            Some(_) if candidate.over <= 0.0 => true,
            Some(kept) => kept.over > 0.0 && candidate.over < kept.over,
        };
        if replace {
            self.0 = Some(candidate);
        }
    }
}

/// 倍率 `gain_db` から始め、トゥルーピークが上限に収まり統合ラウドネスが目標に合うまで調整する。
/// 上限は譲らない（収まった候補のうち最後のものを返す）。収まる候補が無ければ上限を超えた量が最小の候補を返す。
/// 候補ごとに標本を複製せず、倍率の列だけを作って元の標本から測り直す。
fn fit(source: &AudioBlock, spec: &NormalizeSpec, mut gain_db: f32) -> Result<Candidate> {
    let peaks = frame_peaks(&source.samples, usize::from(source.channels));
    let mut margin_db = 0.0f32;
    let mut best = Best::default();
    for _ in 0..MAX_ROUNDS {
        let gain = db_to_gain(gain_db);
        let limiter = limiter_gains(
            peaks.iter().map(|p| *p * gain),
            source.sample_rate,
            db_to_gain(spec.ceiling_dbtp - margin_db),
        );
        let limited = limiter.is_some();
        let envelope = Envelope { gain, limiter };
        let after = envelope.measure(source)?;
        let over = after.true_peak_dbtp - spec.ceiling_dbtp;
        let err = spec.target_lufs - after.integrated_lufs.unwrap_or(spec.target_lufs);
        best.offer(Candidate {
            envelope,
            after,
            limited,
            over,
        });
        if over > 0.0 {
            margin_db += over + PEAK_MARGIN_DB;
            continue;
        }
        if err.abs() <= LUFS_TOLERANCE {
            break;
        }
        gain_db += err;
    }
    best.0
        .ok_or_else(|| MmError::Internal("正規化の候補が無い".into()))
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

    /// 候補ごとに音を複製する参照実装。倍率の列だけで測る実装と同じ結果になることを確かめる。
    fn fit_by_clone(source: &AudioBlock, spec: &NormalizeSpec, mut gain_db: f32) -> AudioBlock {
        let mut margin_db = 0.0f32;
        let mut kept: Option<AudioBlock> = None;
        for _ in 0..MAX_ROUNDS {
            let mut block = source.clone();
            let gain = db_to_gain(gain_db);
            block.samples.iter_mut().for_each(|s| *s *= gain);
            limit(&mut block, db_to_gain(spec.ceiling_dbtp - margin_db));
            let after = measure(&block).unwrap();
            let over = after.true_peak_dbtp - spec.ceiling_dbtp;
            let err = spec.target_lufs - after.integrated_lufs.unwrap_or(spec.target_lufs);
            if over > 0.0 {
                margin_db += over + PEAK_MARGIN_DB;
                kept = kept.or(Some(block));
                continue;
            }
            kept = Some(block);
            if err.abs() <= LUFS_TOLERANCE {
                break;
            }
            gain_db += err;
        }
        kept.unwrap()
    }

    #[test]
    fn 倍率の列で測っても_複製して測った結果と同じ() {
        // MEASURE_CHUNK_FRAMES をまたぐ長さにして、分割の測りも確かめる。
        for src in [peaky(3.0), sine(3.0, 0.05), sine(3.0, 0.9)] {
            let measured = measure(&src).unwrap().integrated_lufs.unwrap();
            let gain_db = SPEC_YT.target_lufs - measured;
            let expected = fit_by_clone(&src, &SPEC_YT, gain_db);
            let mut actual = src.clone();
            DspLoudnessNormalizer
                .normalize(&mut actual, &SPEC_YT)
                .unwrap();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn 倍率の列の測りは複製して測った値と一致する() {
        let src = peaky(3.0);
        let peaks = frame_peaks(&src.samples, 2);
        let gain = db_to_gain(12.0);
        let limiter = limiter_gains(peaks.iter().map(|p| *p * gain), RATE, 0.5);
        assert!(limiter.is_some());
        let envelope = Envelope { gain, limiter };
        let mut cloned = src.clone();
        envelope.apply_in_place(&mut cloned);
        assert_eq!(envelope.measure(&src).unwrap(), measure(&cloned).unwrap());
    }

    fn candidate(over: f32) -> Candidate {
        Candidate {
            envelope: Envelope {
                gain: over,
                limiter: None,
            },
            after: Measurement {
                integrated_lufs: None,
                true_peak_dbtp: 0.0,
            },
            limited: false,
            over,
        }
    }

    #[test]
    fn どの候補も上限を超えるなら_超えた量が最小の候補を返す() {
        let mut best = Best::default();
        for over in [0.9, 0.3, 0.6] {
            best.offer(candidate(over));
        }
        assert_eq!(best.0.unwrap().over, 0.3);
    }

    #[test]
    fn 上限に収まった候補は後のものが優先され_超えた候補に負けない() {
        let mut best = Best::default();
        for over in [0.5, -0.2, 0.1, -0.4, 0.05] {
            best.offer(candidate(over));
        }
        assert_eq!(best.0.unwrap().over, -0.4);
    }

    #[test]
    fn 窓の最小と平均は端でも長さを保つ() {
        let v = [1.0, 0.5, 1.0, 1.0, 1.0];
        let m = window_min(&v, 1);
        assert_eq!(m, vec![0.5, 0.5, 0.5, 1.0, 1.0]);
        assert_eq!(smooth(&m, 1).len(), v.len());
    }
}
