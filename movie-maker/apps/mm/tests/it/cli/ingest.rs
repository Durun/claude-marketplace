//! `mm ingest` を実行ファイル越しに確かめる。ffmpeg の CLI が無い環境ではスキップする（`MM_REQUIRE_FFMPEG=1` なら失敗にする）。

use std::path::Path;
use std::process::{Command, Stdio};

use super::common::{mm_ok, stdout};

fn ffmpeg_available() -> bool {
    let ok = Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    assert!(
        ok || std::env::var_os("MM_REQUIRE_FFMPEG").is_none(),
        "MM_REQUIRE_FFMPEG が立っているのに ffmpeg が使えない"
    );
    ok
}

/// 映像（320x180、2 秒）と音つきのフィクスチャの MP4。
fn make_sample(out: &Path) {
    let status = Command::new("ffmpeg")
        .args(["-hide_banner", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=s=320x180:r=30:d=2")
        .args(["-f", "lavfi", "-i"])
        .arg("sine=frequency=440:sample_rate=48000:duration=2")
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ])
        .args(["-c:a", "aac", "-shortest"])
        .arg(out)
        .status()
        .unwrap();
    assert!(status.success());
}

/// `cache/<種類>/<ハッシュ>/` の中のファイルの数。
fn count_files(cache: &Path, kind: &str) -> usize {
    std::fs::read_dir(cache.join(kind))
        .unwrap()
        .map(|d| std::fs::read_dir(d.unwrap().path()).unwrap().count())
        .sum()
}

#[test]
fn 取り込むと素材が足され_cache_に出力ができて_2_回目は飛ぶ() {
    if !ffmpeg_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("p");
    let sample = tmp.path().join("sample.mp4");
    make_sample(&sample);
    let (d, s) = (dir.to_str().unwrap(), sample.to_str().unwrap());
    mm_ok(&["new", d]);

    let first = stdout(&mm_ok(&["ingest", d, s, "--encoder", "x264"]));
    assert!(first.contains("素材を 1 件足した"), "{first}");
    let cache = dir.join("cache");
    assert_eq!(count_files(&cache, "preview"), 1);
    assert_eq!(count_files(&cache, "audio"), 1);
    assert_eq!(count_files(&cache, "thumbs"), 1);

    mm_ok(&["export", d]);
    let assets: Vec<_> = std::fs::read_dir(dir.join("versions/assets"))
        .unwrap()
        .collect();
    assert_eq!(assets.len(), 1);
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(assets[0].as_ref().unwrap().path()).unwrap())
            .unwrap();
    assert_eq!(json["content_hash"].as_str().unwrap().len(), 64);

    let second = stdout(&mm_ok(&["ingest", d, s, "--encoder", "x264"]));
    assert_eq!(second.matches("スキップした").count(), 3, "{second}");
    assert!(second.contains("既にプロジェクトにある"), "{second}");
    assert!(
        !second.contains("素材を"),
        "2 回目は素材を足さない: {second}"
    );
}
