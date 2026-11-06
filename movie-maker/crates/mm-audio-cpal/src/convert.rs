//! デバイスの形（サンプルレート、チャンネル数）への変換。ミキサーの出力は 48 kHz のステレオなので、
//! デバイスが違う形のときだけ通す。ブロックの境目で位相が途切れないよう、状態を持つ。

/// 線形補間のリサンプラー（インターリーブ）。プレビュー用で、書き出しの変換器とは別。
#[derive(Debug, Clone)]
pub struct Resampler {
    channels: usize,
    /// 出力 1 フレームあたりの入力フレーム数。
    step: f64,
    /// 次の出力の、いまの入力ブロックの先頭からの位置（-1 なら直前のブロックの最後のフレーム）。
    pos: f64,
    last: Vec<f32>,
}

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32, channels: u16) -> Self {
        let channels = usize::from(channels.max(1));
        Self {
            channels,
            step: f64::from(in_rate.max(1)) / f64::from(out_rate.max(1)),
            pos: 0.0,
            last: vec![0.0; channels],
        }
    }

    /// 入力のブロックを変換する。
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        // 同じレートは補間せず、フレーム数を変えない。
        if self.step == 1.0 {
            return input.to_vec();
        }
        let n = input.len() / self.channels;
        let mut out = Vec::with_capacity(((n as f64 / self.step) as usize + 1) * self.channels);
        while self.pos < (n as f64) - 1.0 {
            let i = self.pos.floor();
            let frac = (self.pos - i) as f32;
            let i = i as i64;
            for c in 0..self.channels {
                let a = if i < 0 {
                    self.last[c]
                } else {
                    input[i as usize * self.channels + c]
                };
                let b = input[(i + 1) as usize * self.channels + c];
                out.push(a + (b - a) * frac);
            }
            self.pos += self.step;
        }
        if n > 0 {
            self.pos -= n as f64;
            self.last
                .copy_from_slice(&input[(n - 1) * self.channels..n * self.channels]);
        }
        out
    }
}

/// チャンネル数を合わせる。多い方へは左右だけ入れて残りは無音、少ない方へは平均する。
pub fn remap_channels(input: &[f32], from: u16, to: u16) -> Vec<f32> {
    let (from, to) = (usize::from(from.max(1)), usize::from(to.max(1)));
    if from == to {
        return input.to_vec();
    }
    let mut out = Vec::with_capacity(input.len() / from * to);
    for frame in input.chunks_exact(from) {
        if to == 1 {
            out.push(frame.iter().sum::<f32>() / from as f32);
        } else {
            for c in 0..to {
                out.push(if from == 1 {
                    frame[0]
                } else {
                    frame.get(c).copied().unwrap_or(0.0)
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(frames: usize) -> Vec<f32> {
        (0..frames).flat_map(|i| [i as f32, -(i as f32)]).collect()
    }

    #[test]
    fn 同じレートはそのまま通る() {
        let mut r = Resampler::new(48_000, 48_000, 2);
        let x = ramp(100);
        assert_eq!(r.process(&x), x);
    }

    #[test]
    fn 長さはレートの比に従う() {
        let mut r = Resampler::new(48_000, 44_100, 2);
        let y = r.process(&ramp(4800));
        let frames = y.len() / 2;
        assert!((4410i64 - frames as i64).abs() <= 2, "{frames}");
    }

    #[test]
    fn ブロックを分けても一度に通したのと同じ() {
        let x = ramp(1000);
        let whole = Resampler::new(48_000, 44_100, 2).process(&x);
        let mut split = Resampler::new(48_000, 44_100, 2);
        let mut parts = split.process(&x[..2 * 333]);
        parts.extend(split.process(&x[2 * 333..]));
        assert_eq!(whole.len(), parts.len());
        let worst = whole
            .iter()
            .zip(&parts)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(worst < 1e-3, "{worst}");
    }

    #[test]
    fn ステレオからモノは平均_モノからステレオは複製() {
        assert_eq!(remap_channels(&[1.0, 3.0], 2, 1), vec![2.0]);
        assert_eq!(remap_channels(&[0.5], 1, 2), vec![0.5, 0.5]);
        assert_eq!(remap_channels(&[1.0, 2.0], 2, 4), vec![1.0, 2.0, 0.0, 0.0]);
    }
}
