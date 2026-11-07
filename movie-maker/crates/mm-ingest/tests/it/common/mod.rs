//! 結合テストの共通部分。ffmpeg の CLI で小さな素材を作る。

use std::path::{Path, PathBuf};
use std::process::Command;

use mm_ingest::{CacheLayout, IngestOptions, PreviewEncoder, Tools};

/// ffmpeg が使えればツールを返す。無ければスキップする（`MM_REQUIRE_FFMPEG=1` なら失敗にする）。
pub fn tools() -> Option<Tools> {
    let tools = Tools::default();
    if tools.available() {
        return Some(tools);
    }
    assert!(
        std::env::var_os("MM_REQUIRE_FFMPEG").is_none(),
        "MM_REQUIRE_FFMPEG が立っているのに ffmpeg が使えない"
    );
    eprintln!("ffmpeg が無いのでスキップする");
    None
}

/// 映像（`size`、30fps）と音（440Hz、ステレオ）つきの MP4 を作る。
pub fn make_media(tools: &Tools, out: &Path, size: &str, seconds: u32, with_audio: bool) {
    let mut cmd = Command::new(&tools.ffmpeg);
    cmd.args(["-hide_banner", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=s={size}:r=30:d={seconds}"));
    if with_audio {
        cmd.args(["-f", "lavfi", "-i"]).arg(format!(
            "sine=frequency=440:sample_rate=48000:duration={seconds},aformat=channel_layouts=stereo"
        ));
    }
    cmd.args([
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-pix_fmt",
        "yuv420p",
    ]);
    if with_audio {
        cmd.args(["-c:a", "aac", "-shortest"]);
    }
    let status = cmd.arg(out).status().unwrap();
    assert!(status.success(), "素材を作れなかった");
}

pub fn options(tools: Tools, root: &Path) -> IngestOptions {
    IngestOptions {
        cache: CacheLayout::new(root.join("cache")),
        tools,
        // CI に GPU が無くても通るよう x264 に固定する。
        encoder: PreviewEncoder::X264,
    }
}

/// 320x180、2 秒、映像と音つきのフィクスチャ。
pub fn sample(dir: &Path, tools: &Tools) -> PathBuf {
    let path = dir.join("sample.mp4");
    make_media(tools, &path, "320x180", 2, true);
    path
}
