use std::cell::RefCell;
use std::process::Command;

use mm_doc::Project;
use mm_types::{ColorSpace, Id, RationalTime};

use super::*;

fn state_with(dir: &Path, assets: Vec<Asset>) -> MediaState {
    let shared = Arc::new(Shared::new());
    let mut project = Project::new();
    project.assets = assets;
    shared.publish_doc(Arc::new(project), Id::new("main"));
    MediaState::new(shared, dir.join("cache"))
}

fn asset(path: &str, hash: &str) -> Asset {
    Asset {
        id: Id::new(format!("a_{hash}")),
        path: path.to_owned(),
        content_hash: hash.to_owned(),
        color_space: ColorSpace::bt709(),
        duration: RationalTime::seconds(2),
        fps: None,
        timebase_offset: RationalTime::zero(),
    }
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn 間引きはチャンネルの最大を取り_元より細かくしない() {
    let peaks = [0.1, 0.5, 0.3, 0.2, 0.9, 0.0, 0.4, 0.4];
    assert_eq!(downsample(&peaks, 2, 2), [0.5, 0.9]);
    assert_eq!(downsample(&peaks, 2, 100), [0.5, 0.3, 0.9, 0.4]);
    assert!(downsample(&peaks, 2, 0).is_empty());
    assert!(downsample(&[], 2, 4).is_empty());
}

#[test]
fn 元のファイルが無い素材は_missing_になる() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_with(dir.path(), vec![asset("no/such/clip.mp4", "h1")]);
    let items = state.list();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "clip.mp4");
    assert_eq!(items[0].status, MediaStatus::Missing);
}

#[test]
fn キャッシュの大きさを数え_掃除は許可した名前だけ消す() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_with(dir.path(), vec![]);
    let cache = dir.path().join("cache");
    write(&cache.join("preview/h/540p.mp4"), &[0; 100]);
    write(&cache.join("thumbs/h/thumb.jpg"), &[0; 10]);
    write(&cache.join("ingest/h.transcript.json"), &[0; 5]);

    let info = state.cache_info();
    assert_eq!(info.total_bytes, 115);
    let preview = info.folders.iter().find(|f| f.name == "preview").unwrap();
    assert_eq!(
        (preview.bytes, preview.files, preview.clearable),
        (100, 1, true)
    );
    let ingest = info.folders.iter().find(|f| f.name == "ingest").unwrap();
    assert!(!ingest.clearable);

    assert!(state.clear_cache(&["ingest".into()]).is_err());
    assert!(state.clear_cache(&["../escape".into()]).is_err());
    assert!(cache.join("ingest/h.transcript.json").is_file());

    let freed = state
        .clear_cache(&["preview".into(), "audio".into(), "thumbs".into()])
        .unwrap();
    assert_eq!(freed, 110);
    assert!(!cache.join("preview").exists());
    assert!(cache.join("ingest/h.transcript.json").is_file());
}

fn options(dir: &Path) -> IngestOptions {
    IngestOptions {
        cache: CacheLayout::new(dir.join("cache")),
        tools: Tools::default(),
        encoder: PreviewEncoder::X264,
    }
}

#[test]
fn 取り込みに失敗した素材は飛ばして次へ進む() {
    let dir = tempfile::tempdir().unwrap();
    let events = RefCell::new(Vec::new());
    run_import(
        &options(dir.path()),
        &["no/such/a.mp4".into(), "no/such/b.mp4".into()],
        &HashSet::new,
        &|_| Ok(()),
        &|e| events.borrow_mut().push(e),
    );
    let events = events.into_inner();
    assert_eq!(events.len(), 5);
    assert!(matches!(events[1], ImportEvent::Failed { index: 0, .. }));
    assert!(matches!(events[3], ImportEvent::Failed { index: 1, .. }));
    assert_eq!(
        events[4],
        ImportEvent::Finished {
            added: 0,
            skipped: 0,
            failed: 2
        }
    );
}

/// ffmpeg で 1 秒の映像と音の素材を作る。無ければスキップする（`MM_REQUIRE_FFMPEG=1` なら失敗）。
fn make_clip(dir: &Path) -> Option<PathBuf> {
    let tools = Tools::default();
    if !tools.available() {
        assert!(
            std::env::var_os("MM_REQUIRE_FFMPEG").is_none(),
            "MM_REQUIRE_FFMPEG が立っているのに ffmpeg が使えない"
        );
        eprintln!("ffmpeg が無いのでスキップする");
        return None;
    }
    let out = dir.join("clip.mp4");
    let status = Command::new(&tools.ffmpeg)
        .args(["-hide_banner", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc2=s=320x180:r=30:d=1")
        .args(["-f", "lavfi", "-i"])
        .arg("sine=frequency=440:sample_rate=48000:duration=1")
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ])
        .args(["-c:a", "aac", "-shortest"])
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success());
    Some(out)
}

#[test]
fn 取り込むと_addasset_が当たり_同じ素材の_2_回目は足さない() {
    let dir = tempfile::tempdir().unwrap();
    let Some(clip) = make_clip(dir.path()) else {
        return;
    };
    let path = clip.to_string_lossy().into_owned();
    let applied = RefCell::new(Vec::<Op>::new());
    let events = RefCell::new(Vec::new());
    let known = || {
        applied
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::AddAsset(a) => Some(a.asset.content_hash.clone()),
                _ => None,
            })
            .collect()
    };
    let apply = |op: Op| {
        applied.borrow_mut().push(op);
        Ok(())
    };
    for _ in 0..2 {
        run_import(
            &options(dir.path()),
            std::slice::from_ref(&path),
            &known,
            &apply,
            &|e| events.borrow_mut().push(e),
        );
    }
    assert_eq!(applied.borrow().len(), 1, "2 回目は足さない");
    let finished: Vec<_> = events
        .borrow()
        .iter()
        .filter(|e| matches!(e, ImportEvent::Finished { .. }))
        .cloned()
        .collect();
    assert_eq!(
        finished,
        [
            ImportEvent::Finished {
                added: 1,
                skipped: 0,
                failed: 0
            },
            ImportEvent::Finished {
                added: 0,
                skipped: 1,
                failed: 0
            },
        ]
    );
}

#[test]
fn 取り込んだ素材は一覧で_ready_になり_サムネイルと波形が引ける() {
    let dir = tempfile::tempdir().unwrap();
    let Some(clip) = make_clip(dir.path()) else {
        return;
    };
    let report = mm_ingest::ingest(&options(dir.path()), &clip).unwrap();
    let state = state_with(dir.path(), vec![report.asset.clone()]);
    let item = &state.list()[0];
    assert_eq!(item.status, MediaStatus::Ready);
    assert_eq!(item.kind, MediaKind::Video);
    assert_eq!((item.width, item.height), (Some(320), Some(180)));
    let thumb = state.thumbnail(&item.id).unwrap();
    assert_eq!(&thumb[..2], [0xff, 0xd8], "JPEG の先頭");
    let wave = state.waveform(&item.id, 50).unwrap();
    assert_eq!(wave.peaks.len(), 50);
    assert!(wave.peaks.iter().any(|p| *p > 0.05));
}

#[test]
fn 同じ中身のファイルを_1_回に_2_つ渡すと_2_つ目は足さない() {
    let dir = tempfile::tempdir().unwrap();
    let Some(clip) = make_clip(dir.path()) else {
        return;
    };
    let copy = dir.path().join("copy.mp4");
    std::fs::copy(&clip, &copy).unwrap();
    let paths = [clip, copy].map(|p| p.to_string_lossy().into_owned());
    let applied = RefCell::new(0);
    let events = RefCell::new(Vec::new());
    // 適用がまだ一覧に映っていない（Core の書いた結果を読む側が古い）ときと同じ。
    run_import(
        &options(dir.path()),
        &paths,
        &HashSet::new,
        &|_| {
            *applied.borrow_mut() += 1;
            Ok(())
        },
        &|e| events.borrow_mut().push(e),
    );
    assert_eq!(*applied.borrow(), 1);
    assert!(events.borrow().contains(&ImportEvent::Finished {
        added: 1,
        skipped: 1,
        failed: 0
    }));
}

#[test]
fn 取り込み中は掃除を断る() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_with(dir.path(), vec![]);
    let busy = state.importing.lock().unwrap();
    let refused = state.clear_cache(&["preview".into()]);
    assert_eq!(refused, Err("取り込み中は掃除できない".into()));
    drop(busy);
    assert!(state.clear_cache(&["preview".into()]).is_ok());
}

#[test]
fn 選択かドロップで渡されていないパスは取り込まない() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_with(dir.path(), vec![]);
    let picked = dir.path().join("picked.mp4");
    state.offer([picked.clone()]);
    let events = RefCell::new(Vec::new());
    let emit = |e| events.borrow_mut().push(e);
    let stray = "C:/Windows/win.ini".to_owned();
    let refused = state.import(&[picked.to_string_lossy().into_owned(), stray], &emit);
    assert!(refused.unwrap_err().contains("win.ini"));
    assert!(events.borrow().is_empty(), "1 つも取り込まない");
    let accepted = state.import(&[picked.to_string_lossy().into_owned()], &emit);
    assert!(
        accepted.is_ok(),
        "選んだパスは受ける（無いファイルは素材ごとの失敗）"
    );
}
