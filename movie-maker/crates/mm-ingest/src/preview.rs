//! プレビュー動画（小さく作り直した動画）。編集中のスクラブに使う、短い GOP の H.264。

use std::path::Path;
use std::process::Command;

use mm_types::Result;

use crate::layout::{ensure_parent, partial_path};
use crate::probe::MediaProbe;
use crate::tools::{run, Tools};

/// プレビュー動画の高さ。元がこれより小さければ元のまま。
pub const PREVIEW_HEIGHT: u32 = 540;
/// キーフレームの間隔。短いほどスクラブの飛び先からの読み進めが短い。
const GOP_FRAMES: u32 = 12;
/// 音のビットレート。
const AUDIO_BITRATE: &str = "128k";

/// プレビュー動画のエンコーダー。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewEncoder {
    /// NVENC を試し、失敗したら x264。
    #[default]
    Auto,
    /// NVENC（`h264_nvenc`）だけ。
    Nvenc,
    /// x264（`libx264`）だけ。
    X264,
}

impl PreviewEncoder {
    fn candidates(self) -> &'static [&'static str] {
        match self {
            Self::Auto => &["h264_nvenc", "libx264"],
            Self::Nvenc => &["h264_nvenc"],
            Self::X264 => &["libx264"],
        }
    }
}

/// 作ったプレビュー動画。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewResult {
    /// 使ったエンコーダー。
    pub encoder: &'static str,
}

/// プレビュー動画を `out` に作る。完成してから置く。
pub fn make_preview(
    tools: &Tools,
    input: &Path,
    probe: &MediaProbe,
    out: &Path,
    encoder: PreviewEncoder,
) -> Result<PreviewResult> {
    ensure_parent(out)?;
    let part = partial_path(out);
    let mut last_error = None;
    for name in encoder.candidates() {
        let mut cmd = preview_command(tools, input, probe, &part, name);
        match run(&mut cmd, &format!("プレビュー動画（{name}）")) {
            Ok(_) => {
                std::fs::rename(&part, out)?;
                return Ok(PreviewResult { encoder: name });
            }
            Err(e) => last_error = Some(e),
        }
    }
    let _ = std::fs::remove_file(&part);
    Err(last_error.expect("エンコーダーの候補は 1 つ以上ある"))
}

/// 引数を組む。エンコーダーごとに品質の指定の名前が違う。
fn preview_command(
    tools: &Tools,
    input: &Path,
    probe: &MediaProbe,
    out: &Path,
    encoder: &str,
) -> Command {
    let mut cmd = tools.ffmpeg_command();
    cmd.arg("-i").arg(input);
    cmd.args(["-map", "0:v:0", "-vf"]).arg(format!(
        "scale=-2:'trunc(min({PREVIEW_HEIGHT},ih)/2)*2',format=yuv420p"
    ));
    cmd.args(["-c:v", encoder, "-g", &GOP_FRAMES.to_string(), "-bf", "0"]);
    if encoder == "h264_nvenc" {
        cmd.args(["-preset", "p4", "-rc", "vbr", "-cq", "30", "-b:v", "0"]);
    } else {
        cmd.args(["-preset", "veryfast", "-crf", "28"]);
    }
    if probe.audio.is_some() {
        cmd.args(["-map", "0:a:0", "-c:a", "aac", "-b:a", AUDIO_BITRATE]);
    }
    cmd.args(["-movflags", "+faststart", "-f", "mp4"]).arg(out);
    cmd
}
