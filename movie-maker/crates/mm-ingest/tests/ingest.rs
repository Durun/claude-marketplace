//! 取り込みの結合テスト。ffmpeg の CLI で小さな素材を作って取り込む。
//! ffmpeg が無い環境では飛ばす（`MM_REQUIRE_FFMPEG=1` なら失敗にする）。

use std::process::Command;

use mm_ingest::{asset_id, ingest, probe, ArtifactState};
use mm_ingest_api::SidecarBody;
use mm_types::MmError;

mod common;

use common::{make_media, options, sample, tools};

#[test]
fn 取り込むとプレビュー動画と波形とサムネイルができて_2_回目は飛ぶ() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().unwrap();
    let input = sample(dir.path(), &tools);
    let opts = options(tools, dir.path());
    let first = ingest(&opts, &input).unwrap();
    let second = ingest(&opts, &input).unwrap();

    for (name, art) in [
        ("プレビュー動画", &first.preview),
        ("サムネイル", &first.thumbnail),
        ("波形", &first.waveform),
    ] {
        let art = art.as_ref().unwrap_or_else(|| panic!("{name} が無い"));
        assert_eq!(art.state, ArtifactState::Created, "{name}");
        assert!(art.path.metadata().unwrap().len() > 0, "{name} が空");
    }
    assert!(second.all_cached(), "2 回目は全部飛ぶ");
    assert_eq!(first.asset, second.asset);
    assert_eq!(first.asset.id, asset_id(&first.asset.content_hash));
    assert_eq!(first.asset.duration.to_f64().round() as i64, 2);
}

#[test]
fn 波形のピークがサイン波の振幅に近い() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().unwrap();
    let input = sample(dir.path(), &tools);
    let opts = options(tools, dir.path());
    let report = ingest(&opts, &input).unwrap();
    let path = report.waveform.unwrap().path;
    let sidecar = mm_ingest::sidecar::read_sidecar(&path, &report.asset.content_hash).unwrap();
    let SidecarBody::Waveform(w) = sidecar.body else {
        panic!("波形でない");
    };
    assert_eq!((w.channels, w.peaks_per_second), (2, 100));
    // 2 秒 × 100 ピーク × 2 チャンネル（AAC の先頭と末尾の端数で数ピークずれてよい）。
    assert!((396..=410).contains(&w.peaks.len()), "{}", w.peaks.len());
    let mid = w.peaks[w.peaks.len() / 2];
    assert!((0.07..=0.11).contains(&mid), "{mid}");
}

#[test]
fn プレビュー動画は高さ_540_に縮み_元より小さいと元のまま() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("big.mp4");
    make_media(&tools, &big, "1920x1080", 1, false);
    let opts = options(tools.clone(), dir.path());
    let report = ingest(&opts, &big).unwrap();
    let preview = probe(&tools, &report.preview.unwrap().path).unwrap();
    let v = preview.video.unwrap();
    assert_eq!((v.width, v.height), (960, 540));
    assert!(report.waveform.is_none(), "音が無ければ波形は作らない");

    let small = sample(dir.path(), &tools);
    let report = ingest(&opts, &small).unwrap();
    let v = probe(&tools, &report.preview.unwrap().path)
        .unwrap()
        .video
        .unwrap();
    assert_eq!((v.width, v.height), (320, 180));
}

#[test]
fn 映像の無い素材は波形だけ作る() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("tone.wav");
    let status = Command::new(&tools.ffmpeg)
        .args(["-hide_banner", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("sine=frequency=440:sample_rate=48000:duration=1")
        .arg(&wav)
        .status()
        .unwrap();
    assert!(status.success());
    let report = ingest(&options(tools, dir.path()), &wav).unwrap();
    assert!(report.preview.is_none() && report.thumbnail.is_none());
    assert!(report.waveform.is_some());
    assert_eq!(report.asset.fps, None);
}

#[test]
fn 壊れた素材と無いファイルはエラーでクラッシュしない() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().unwrap();
    let broken = dir.path().join("broken.mp4");
    std::fs::write(&broken, b"not a movie").unwrap();
    let opts = options(tools, dir.path());
    assert!(ingest(&opts, &broken).is_err());
    let missing = dir.path().join("missing.mp4");
    assert!(matches!(ingest(&opts, &missing), Err(MmError::NotFound(_))));
}

#[test]
fn 文字起こしの付属ファイルがあれば区間の数を報告する() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().unwrap();
    let input = sample(dir.path(), &tools);
    let opts = options(tools, dir.path());
    let first = ingest(&opts, &input).unwrap();
    assert_eq!(first.transcript_segments, None);

    let hash = &first.asset.content_hash;
    let sidecar = serde_json::json!({
        "schema_version": 1, "content_hash": hash, "producer": "test", "kind": "transcript",
        "language": "ja",
        "segments": [{"start": {"num": 0, "den": 1}, "end": {"num": 1, "den": 1}, "text": "テスト"}]
    });
    let path = opts.cache.sidecar(hash, "transcript");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, sidecar.to_string()).unwrap();
    assert_eq!(ingest(&opts, &input).unwrap().transcript_segments, Some(1));
}
