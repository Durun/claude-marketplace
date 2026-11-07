//! 2 本の動画の比べ方。映像は FFmpeg の `ssim` と `psnr` のフィルター、音は PCM に戻して数える。

use std::path::Path;
use std::process::Command;

/// 音を比べるサンプルレート。
pub const RATE: usize = 48_000;
/// 音の大きさの包絡を取る区間（サンプル）。50 ms。
const ENVELOPE_WINDOW: usize = RATE / 20;
/// 包絡を比べる下限（dBFS）。これより小さい区間は無音として比べない。
const ENVELOPE_FLOOR_DB: f64 = -45.0;
/// 音の時刻のずれを探す幅（サンプル）。±50 ms。
const MAX_LAG: usize = RATE / 20;
/// ずれを探すのに使う長さ（サンプル）。1 秒。
const LAG_PROBE: usize = RATE;
/// 0 の対数を避ける小さい値。
const TINY: f64 = 1e-12;
/// `astats` の出力のピークの行の見出し。
const PEAK_KEY: &str = "Peak level dB:";

/// 区間の映像の比べ。
#[derive(Debug, Clone, Copy)]
pub struct Picture {
    pub frames: usize,
    pub ssim_mean: f64,
    pub ssim_min: f64,
    pub psnr_mean: f64,
}

/// 区間の音の比べ。
#[derive(Debug, Clone, Copy)]
pub struct Sound {
    /// 書き出しを参照に合わせる倍率（dB）。区間だけの書き出しはラウドネスの正規化が区間で決まるので、比べる前に合わせる。
    pub gain_db: f64,
    /// 参照に対する時刻のずれ（ミリ秒、正は書き出しが遅い）。
    pub lag_ms: f64,
    /// 倍率とずれを合わせた後の、参照と差の比（dB）。
    pub snr_db: f64,
    /// 50 ms ごとの大きさの差の平均（dB）。参照が -45 dBFS より大きい区間だけ。
    pub envelope_db: f64,
    /// 書き出しの音のサンプルのピーク（dBFS。[`peak_dbfs`] で数えた値）。
    pub peak_dbfs: f64,
}

fn ffmpeg() -> Command {
    let mut c = Command::new("ffmpeg");
    c.args(["-v", "error", "-nostdin", "-y"]);
    c
}

fn run(mut cmd: Command) -> Vec<u8> {
    let out = cmd.output().expect("ffmpeg を実行できる");
    assert!(
        out.status.success(),
        "ffmpeg が成功する: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

/// 書き出し `test` の全体と、参照 `reference` の `[start, start + len)` の映像を比べる。
pub fn picture(test: &Path, reference: &Path, start: f64, len: f64, work: &Path) -> Picture {
    let ssim_log = work.join("ssim.log");
    let psnr_log = work.join("psnr.log");
    let graph = format!(
        "[0:v]split[a0][a1];[1:v]split[b0][b1];[a0][b0]ssim=stats_file='{}';[a1][b1]psnr=stats_file='{}'",
        filter_path(&ssim_log),
        filter_path(&psnr_log)
    );
    let mut cmd = ffmpeg();
    cmd.arg("-i").arg(test);
    cmd.args(["-ss", &start.to_string(), "-t", &len.to_string()]);
    cmd.arg("-i").arg(reference);
    cmd.args(["-lavfi", &graph, "-f", "null", "-"]);
    run(cmd);
    let ssim = stats(&ssim_log, "All:");
    let psnr = stats(&psnr_log, "psnr_avg:");
    Picture {
        frames: ssim.len(),
        ssim_mean: mean(&ssim),
        ssim_min: ssim.iter().copied().fold(f64::INFINITY, f64::min),
        psnr_mean: mean(&psnr),
    }
}

/// フィルターの引数に書くパス。Windows のドライブの `:` を逃がす。
fn filter_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").replace(':', "\\:")
}

/// FFmpeg の統計のファイルから、`key` の後ろの数を行ごとに読む。`inf`（同一）は 100 dB にする。
fn stats(path: &Path, key: &str) -> Vec<f64> {
    let text = std::fs::read_to_string(path).expect("統計のファイルがある");
    text.lines()
        .filter_map(|line| {
            let rest = &line[line.find(key)? + key.len()..];
            let token = rest.split_whitespace().next()?;
            Some(if token == "inf" {
                100.0
            } else {
                token.parse().ok()?
            })
        })
        .collect()
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len().max(1) as f64
}

/// 区間の音をモノラルの 48 kHz の f32 にする。入力の `-ss` は AAC で最大 1 パケット（約 21 ms）ずれるので使わず、
/// 先頭からデコードして `atrim` で切る。
pub fn pcm(path: &Path, start: f64, len: f64) -> Vec<f32> {
    let mut cmd = ffmpeg();
    cmd.arg("-i").arg(path);
    let trim = format!("atrim=start={start}:duration={len},asetpts=PTS-STARTPTS");
    cmd.args([
        "-af",
        &trim,
        "-ac",
        "1",
        "-ar",
        &RATE.to_string(),
        "-f",
        "f32le",
        "-",
    ]);
    run(cmd)
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

/// 音のサンプルのピーク（dBFS、全部のチャンネルの最大）。片方のチャンネルだけの短いクリックも拾うよう、
/// モノラルにせず浮動小数のまま `astats` で数える。
pub fn peak_dbfs(path: &Path) -> f64 {
    // astats はログの info で書くので、ほかの呼び出しと違い -v info で実行する
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "info", "-nostdin", "-i"]).arg(path);
    cmd.args(["-af", "aformat=sample_fmts=flt,astats", "-f", "null", "-"]);
    let out = cmd.output().expect("ffmpeg を実行できる");
    assert!(out.status.success(), "ffmpeg が成功する");
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter_map(|line| {
            let rest = &line[line.find(PEAK_KEY)? + PEAK_KEY.len()..];
            rest.trim().parse::<f64>().ok()
        })
        .fold(f64::NEG_INFINITY, f64::max)
}

/// 書き出しの音 `test` を参照 `reference` と比べる。`peak_dbfs` は書き出しのピーク（[`peak_dbfs`]）。
pub fn sound(test: &[f32], reference: &[f32], peak_dbfs: f64) -> Sound {
    let lag = best_lag(test, reference);
    let (t, r) = aligned(test, reference, lag);
    let dot: f64 = t
        .iter()
        .zip(r)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum();
    let tt: f64 = t.iter().map(|a| f64::from(*a).powi(2)).sum();
    let gain = if tt > TINY { dot / tt } else { 1.0 };
    let signal: f64 = r.iter().map(|b| f64::from(*b).powi(2)).sum();
    let noise: f64 = t
        .iter()
        .zip(r)
        .map(|(a, b)| (f64::from(*a) * gain - f64::from(*b)).powi(2))
        .sum();
    Sound {
        gain_db: 20.0 * gain.abs().max(TINY).log10(),
        lag_ms: lag as f64 * 1000.0 / RATE as f64,
        snr_db: 10.0 * ((signal + TINY) / (noise + TINY)).log10(),
        envelope_db: envelope_error(t, r, gain),
        peak_dbfs,
    }
}

/// 2 本を `lag`（正は書き出しが遅い）だけずらして、重なる所を返す。
fn aligned<'a>(test: &'a [f32], reference: &'a [f32], lag: isize) -> (&'a [f32], &'a [f32]) {
    let (t, r) = if lag >= 0 {
        (&test[lag as usize..], reference)
    } else {
        (test, &reference[lag.unsigned_abs()..])
    };
    let n = t.len().min(r.len());
    (&t[..n], &r[..n])
}

/// 中ほどの 1 秒で相関が最大になるずれ。
fn best_lag(test: &[f32], reference: &[f32]) -> isize {
    let n = test.len().min(reference.len());
    if n < LAG_PROBE + 2 * MAX_LAG {
        return 0;
    }
    let from = (n - LAG_PROBE) / 2;
    let r = &reference[from..from + LAG_PROBE];
    let score = |lag: isize| -> f64 {
        let t = &test[(from as isize + lag) as usize..][..LAG_PROBE];
        t.iter()
            .zip(r)
            .map(|(a, b)| f64::from(*a) * f64::from(*b))
            .sum()
    };
    let max = MAX_LAG as isize;
    (-max..=max)
        .max_by(|a, b| score(*a).total_cmp(&score(*b)))
        .unwrap_or(0)
}

fn rms_db(x: &[f32], gain: f64) -> f64 {
    let p = x
        .iter()
        .map(|v| (f64::from(*v) * gain).powi(2))
        .sum::<f64>()
        / x.len().max(1) as f64;
    10.0 * (p + TINY).log10()
}

fn envelope_error(test: &[f32], reference: &[f32], gain: f64) -> f64 {
    let diffs: Vec<f64> = test
        .chunks_exact(ENVELOPE_WINDOW)
        .zip(reference.chunks_exact(ENVELOPE_WINDOW))
        .filter_map(|(t, r)| {
            let rr = rms_db(r, 1.0);
            (rr > ENVELOPE_FLOOR_DB).then(|| (rms_db(t, gain) - rr).abs())
        })
        .collect();
    mean(&diffs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(len: usize, amp: f32) -> Vec<f32> {
        (0..len)
            .map(|i| amp * (i as f32 * 0.05).sin() * (1.0 + (i as f32 * 0.0007).sin()))
            .collect()
    }

    #[test]
    fn 倍率とずれを合わせれば同じ音は差が無い() {
        let r = tone(RATE * 2, 0.5);
        let mut t = vec![0.0; 48];
        t.extend(r.iter().map(|v| v * 0.5));
        let s = sound(&t, &r, 0.0);
        assert!((s.lag_ms - 1.0).abs() < 1e-9, "{s:?}");
        assert!((s.gain_db - 6.0206).abs() < 0.01, "{s:?}");
        assert!(s.snr_db > 60.0, "{s:?}");
        assert!(s.envelope_db < 0.01, "{s:?}");
    }
}
