//! サムネイル。素材の中ほどの 1 フレームを JPEG にする。

use std::path::Path;

use mm_types::Result;

use crate::layout::{ensure_parent, partial_path};
use crate::probe::MediaProbe;
use crate::tools::{run, Tools};

/// サムネイルの幅。
const THUMBNAIL_WIDTH: u32 = 320;
/// 1 フレームを取る位置（長さに対する割合の分子 / 分母）。先頭は黒いことが多いので中ほどにする。
const POSITION_NUM: f64 = 1.0;
const POSITION_DEN: f64 = 3.0;
/// JPEG の品質（ffmpeg の `-q:v`。小さいほど高画質）。
const JPEG_QUALITY: &str = "4";

/// サムネイルを `out` に作る。
pub fn make_thumbnail(tools: &Tools, input: &Path, probe: &MediaProbe, out: &Path) -> Result<()> {
    ensure_parent(out)?;
    let part = partial_path(out);
    let at = probe.duration.to_f64() * POSITION_NUM / POSITION_DEN;
    let mut cmd = tools.ffmpeg_command();
    cmd.args(["-ss", &format!("{at:.3}")]).arg("-i").arg(input);
    cmd.args(["-map", "0:v:0", "-frames:v", "1", "-vf"])
        .arg(format!("scale={THUMBNAIL_WIDTH}:-2"));
    cmd.args(["-q:v", JPEG_QUALITY, "-f", "image2"]).arg(&part);
    run(&mut cmd, "サムネイル")?;
    std::fs::rename(&part, out)?;
    Ok(())
}
