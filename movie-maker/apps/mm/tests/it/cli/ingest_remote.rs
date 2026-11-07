//! アプリが書き手のとき、`mm ingest` が素材の追加を `/ops`、文字起こしを `/transcripts` で転送する。
//! ffmpeg の CLI が無い環境ではスキップする（`MM_REQUIRE_FFMPEG=1` なら失敗にする）。

use std::path::Path;
use std::process::{Command, Stdio};

use mm_doc::Project;
use mm_serve::{Client, Core, Server};
use mm_store::Store;
use mm_types::AssetId;
use serde_json::{json, Value};

use super::common::{mm, stderr, stdout};

const NOW: i64 = 1_700_000_000_000;

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

/// フィクスチャの文字起こしを、素材のコンテンツハッシュに付け替えて `cache/ingest/` に置く。
fn place_transcript(dir: &Path, media: &Path) {
    let hash = mm_ingest::hash_file(media).unwrap();
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ingest/transcript-ja.json");
    let text = std::fs::read_to_string(fixture).unwrap();
    let mut sidecar: Value = serde_json::from_str(&text).unwrap();
    sidecar["content_hash"] = json!(hash);
    let path = mm_ingest::CacheLayout::new(dir.join("cache")).sidecar(&hash, "transcript");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, sidecar.to_string()).unwrap();
}

#[test]
fn アプリが書き手でも_mm_ingest_は素材と文字起こしを_http_で送る() {
    if !ffmpeg_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("p");
    let store = Store::create(&dir, &Project::new(), NOW).unwrap();
    let server = Server::start(Core::new(store, None)).unwrap();
    let client = Client::new(server.port(), server.token());
    let sample = tmp.path().join("sample.mp4");
    make_sample(&sample);
    place_transcript(&dir, &sample);
    let (d, s) = (dir.to_str().unwrap(), sample.to_str().unwrap());

    let out = mm(&["ingest", d, s, "--encoder", "x264"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("アプリへ転送"), "{text}");
    assert!(text.contains("文字起こし"), "{text}");

    let project = client.get("/project").unwrap().json().unwrap();
    assert_eq!(project["version"], 1);
    let asset = project["project"]["assets"][0]["id"].as_str().unwrap();
    let reader = Store::open_read_only(&dir).unwrap();
    assert!(reader.has_transcript(&AssetId::new(asset)).unwrap());

    // 同じ素材をもう一度渡しても、素材は足されず、文字起こしだけが入り直る。
    let again = mm(&["ingest", d, s, "--encoder", "x264"]);
    assert!(again.status.success(), "{}", stderr(&again));
    assert_eq!(
        client.get("/project").unwrap().json().unwrap()["version"],
        1
    );
}
