//! 波形。音を低いレートの f32 に落として標準出力で受け、一定の窓ごとの最大振幅（ピーク）にする。
//! 全部のサンプルをメモリに載せない（長い素材でも一定のメモリ）。

use std::io::Read;
use std::path::Path;
use std::process::Stdio;

use mm_ingest_api::{Sidecar, SidecarBody, Waveform, SIDECAR_SCHEMA_VERSION};
use mm_types::{MmError, Result};

use crate::layout::{ensure_parent, partial_path};
use crate::tools::{failure_message, spawn_error, Tools};
use crate::PRODUCER;

/// 1 秒あたりのピークの数。
pub const PEAKS_PER_SECOND: u32 = 100;
/// ピークを取る前に落とすサンプルレート。窓が整数のサンプル数になるよう `PEAKS_PER_SECOND` の倍数にする。
const SAMPLE_RATE: u32 = 8000;
/// 標準出力を読む 1 回の大きさ。
const READ_CHUNK: usize = 64 * 1024;
/// ピークの丸め。JSON の大きさを抑える（4 桁）。
const PEAK_SCALE: f32 = 10_000.0;

/// インターリーブした f32le のサンプルを、チャンネルごとの窓の最大振幅にする。
#[derive(Debug)]
pub struct PeakAccumulator {
    channels: usize,
    window: usize,
    current: Vec<f32>,
    filled: usize,
    peaks: Vec<f32>,
    carry: Vec<u8>,
}

impl PeakAccumulator {
    /// `window` は 1 つのピークにまとめるフレーム数（1 フレーム = 全チャンネルの 1 サンプルずつ）。
    pub fn new(channels: usize, window: usize) -> Self {
        Self {
            channels,
            window,
            current: vec![0.0; channels],
            filled: 0,
            peaks: Vec::new(),
            carry: Vec::new(),
        }
    }

    /// 読んだバイト列を足す。フレームの途中で切れた分は次に持ち越す。
    pub fn push(&mut self, bytes: &[u8]) {
        self.carry.extend_from_slice(bytes);
        let frame_bytes = self.channels * 4;
        let usable = self.carry.len() - self.carry.len() % frame_bytes;
        let data: Vec<u8> = self.carry.drain(..usable).collect();
        for frame in data.chunks_exact(frame_bytes) {
            for (ch, s) in frame.chunks_exact(4).enumerate() {
                let v = f32::from_le_bytes([s[0], s[1], s[2], s[3]]).abs().min(1.0);
                self.current[ch] = self.current[ch].max(v);
            }
            self.filled += 1;
            if self.filled == self.window {
                self.flush_window();
            }
        }
    }

    fn flush_window(&mut self) {
        self.peaks.extend(
            self.current
                .iter()
                .map(|v| (v * PEAK_SCALE).round() / PEAK_SCALE),
        );
        self.current.fill(0.0);
        self.filled = 0;
    }

    /// 端数の窓も 1 つのピークにして、インターリーブしたピークを返す。
    pub fn finish(mut self) -> Vec<f32> {
        if self.filled > 0 {
            self.flush_window();
        }
        self.peaks
    }
}

/// 波形の付属ファイルを `out` に作る。
pub fn make_waveform(
    tools: &Tools,
    input: &Path,
    content_hash: &str,
    channels: u16,
    out: &Path,
) -> Result<()> {
    let peaks = decode_peaks(tools, input, channels)?;
    let sidecar = Sidecar {
        schema_version: SIDECAR_SCHEMA_VERSION,
        content_hash: content_hash.to_owned(),
        producer: PRODUCER.to_owned(),
        body: SidecarBody::Waveform(Waveform {
            peaks_per_second: PEAKS_PER_SECOND,
            channels,
            peaks,
        }),
    };
    ensure_parent(out)?;
    let part = partial_path(out);
    std::fs::write(&part, serde_json::to_vec(&sidecar)?)?;
    std::fs::rename(&part, out)?;
    Ok(())
}

fn decode_peaks(tools: &Tools, input: &Path, channels: u16) -> Result<Vec<f32>> {
    let what = "波形";
    let mut cmd = tools.ffmpeg_command();
    cmd.arg("-i").arg(input);
    cmd.args(["-map", "0:a:0", "-vn", "-ac", &channels.to_string()]);
    cmd.args(["-ar", &SAMPLE_RATE.to_string(), "-f", "f32le", "-"]);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| spawn_error(&cmd, what, &e))?;
    let mut stdout = child.stdout.take().expect("stdout を piped にした");
    // 標準エラーを読まずに待つと、出力が詰まって ffmpeg が止まる。別スレッドで読む。
    let mut stderr = child.stderr.take().expect("stderr を piped にした");
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });
    let mut acc = PeakAccumulator::new(
        usize::from(channels),
        (SAMPLE_RATE / PEAKS_PER_SECOND) as usize,
    );
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        let n = stdout.read(&mut buf)?;
        if n == 0 {
            break;
        }
        acc.push(&buf[..n]);
    }
    let status = child.wait()?;
    let stderr = err_reader
        .join()
        .map_err(|_| MmError::Internal("標準エラーを読むスレッドが落ちた".into()))?;
    if !status.success() {
        return Err(MmError::Corrupt(failure_message(
            what,
            &stderr,
            status.code(),
        )));
    }
    Ok(acc.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(samples: &[f32]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    #[test]
    fn 窓ごとの最大振幅をチャンネルごとに取る() {
        let mut acc = PeakAccumulator::new(2, 2);
        // 2 フレームで 1 窓。左は 0.5、-0.25、右は 0.1、-0.8。
        acc.push(&bytes(&[0.5, 0.1, -0.25, -0.8]));
        // 端数の窓（1 フレーム）。
        acc.push(&bytes(&[0.3, 0.0]));
        assert_eq!(acc.finish(), vec![0.5, 0.8, 0.3, 0.0]);
    }

    #[test]
    fn フレームの途中で切れたバイトを持ち越す() {
        let all = bytes(&[0.5, 0.2, 0.25, 0.9]);
        let mut acc = PeakAccumulator::new(2, 2);
        for chunk in all.chunks(3) {
            acc.push(chunk);
        }
        assert_eq!(acc.finish(), vec![0.5, 0.9]);
    }

    #[test]
    fn 範囲を超えた振幅は_1_に収める() {
        let mut acc = PeakAccumulator::new(1, 1);
        acc.push(&bytes(&[3.0]));
        assert_eq!(acc.finish(), vec![1.0]);
    }
}
