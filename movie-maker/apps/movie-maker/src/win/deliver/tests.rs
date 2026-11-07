use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use mm_doc::{
    Cut, Layer, Node, Sequence, SubtitleOutput, FOOTAGE_KIND, FOOTAGE_PARAM, SEQUENCE_KIND,
    SEQUENCE_PARAM,
};
use mm_runtime::Backend;
use mm_store::{TranscriptInput, WordInput};
use mm_testkit::decode::SampleMedia;
use mm_testkit::fixtures::vertical_project;
use mm_types::{ParamValue, RationalTime};

use super::*;

const WAIT: Duration = Duration::from_secs(60);

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("mm_deliver_{name}_{}", std::process::id()))
}

/// 出力先が別の 2 つのレンダーターゲット（`mp4`、`second`）を持つデータを開いた状態。
fn state(dir: &std::path::Path) -> Arc<DeliverState> {
    let (mut project, comp, _) = vertical_project(&SampleMedia::default(), &dir.join("a.json"));
    let mut second = project.render_targets[0].clone();
    second.id = Id::new("second");
    second.output_path = dir.join("b.json").to_string_lossy().into_owned();
    project.render_targets.push(second);
    let shared = Arc::new(Shared::new());
    shared.publish_doc(Arc::new(project), comp);
    Arc::new(DeliverState::new(shared, &dir.join("project.json")))
}

fn request(targets: &[&str]) -> StartRequest {
    StartRequest {
        targets: targets.iter().map(|t| (*t).to_owned()).collect(),
        ..StartRequest::default()
    }
}

/// 状態を送るだけの `Emit`。
fn collect() -> (Emit, Receiver<ExportStatus>) {
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let emit: Emit = Arc::new(move |s| {
        let _ = tx.lock().unwrap().send(s.clone());
    });
    (emit, rx)
}

/// 止まった（`running` が偽の）状態まで読む。
fn finished(rx: &Receiver<ExportStatus>) -> ExportStatus {
    loop {
        let s = rx.recv_timeout(WAIT).expect("書き出しが終わらない");
        if !s.running {
            return s;
        }
    }
}

#[test]
fn キューのレンダーターゲットを順に書き出し_書き出したファイルだけフォルダーを開ける() {
    let dir = temp("order");
    let state = state(&dir);
    let (emit, rx) = collect();
    state
        .start(request(&["second", "mp4"]), RuntimeConfig::sample(), emit)
        .unwrap();
    let last = finished(&rx);
    let targets: Vec<_> = last.items.iter().map(|i| i.target.as_str()).collect();
    assert_eq!(targets, ["second", "mp4"], "依頼の順");
    assert!(last.items.iter().all(|i| i.state == ItemState::Done));
    assert!(last
        .items
        .iter()
        .all(|i| i.frames_total > 0 && i.frames_done == i.frames_total));
    assert!(dir.join("a.json").exists() && dir.join("b.json").exists());
    let a = dir.join("a.json");
    assert_eq!(state.revealable(&a.to_string_lossy()).unwrap(), a);
    assert!(state.revealable("C:/Windows/notepad.exe").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 同じ出力先を指す組と_無い対象は始める前に拒否する() {
    let dir = temp("reject");
    let state = state(&dir);
    let (emit, _rx) = collect();
    let mut project = (*state.project().unwrap()).clone();
    project.render_targets[1].output_path = project.render_targets[0].output_path.clone();
    let comp = state.shared.doc().unwrap().comp;
    state.shared.publish_doc(Arc::new(project), comp);
    let config = RuntimeConfig::sample;
    let e = state
        .start(request(&["mp4", "second"]), config(), emit.clone())
        .unwrap_err();
    assert!(e.contains("同じ出力先"), "{e}");
    assert!(state
        .start(request(&["none"]), config(), emit.clone())
        .is_err());
    assert!(state.start(request(&[]), config(), emit).is_err());
    assert!(!state.status().running, "拒否したときはキューを作らない");
}

#[test]
fn 取り消すと書き出し中の分を止め_残りは書き出さず_一時ファイルを残さない() {
    let dir = temp("cancel");
    let state = state(&dir);
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let canceller = state.clone();
    let emit: Emit = Arc::new(move |s| {
        if s.items.first().is_some_and(|i| i.frames_done > 0) {
            canceller.cancel();
        }
        let _ = tx.lock().unwrap().send(s.clone());
    });
    state
        .start(request(&["mp4", "second"]), RuntimeConfig::sample(), emit)
        .unwrap();
    let last = finished(&rx);
    assert!(
        last.items.iter().all(|i| i.state == ItemState::Cancelled),
        "{last:?}"
    );
    let leftovers = std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0);
    assert_eq!(leftovers, 0, "出力も一時ファイルも残らない");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 書き出し中にもう一度始めようとすると拒否する() {
    let dir = temp("busy");
    let state = state(&dir);
    let (gate_tx, gate_rx) = channel::<()>();
    let (seen_tx, seen_rx) = channel();
    let gate_rx = Mutex::new(gate_rx);
    let seen_tx = Mutex::new(seen_tx);
    let emit: Emit = Arc::new(move |s| {
        let _ = seen_tx.lock().unwrap().send(s.clone());
        let first = &s.items[0];
        if first.state == ItemState::Running && first.frames_done == 0 {
            let _ = gate_rx.lock().unwrap().recv_timeout(WAIT);
        }
    });
    state
        .start(request(&["mp4"]), RuntimeConfig::sample(), emit.clone())
        .unwrap();
    loop {
        let s = seen_rx.recv_timeout(WAIT).unwrap();
        if s.items[0].state == ItemState::Running {
            break;
        }
    }
    let e = state
        .start(request(&["second"]), RuntimeConfig::sample(), emit)
        .unwrap_err();
    assert_eq!(e, "書き出し中");
    gate_tx.send(()).unwrap();
    finished(&seen_rx);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 一覧にはレンダーターゲットとコンポジションが出る() {
    let dir = temp("overview");
    let o = state(&dir).overview().unwrap();
    assert_eq!(o.targets.len(), 2);
    assert_eq!(o.compositions[0].id, "main");
    assert!(o.compositions[0].duration > 0.0);
}

/// 字幕を出す（sidecar）レンダーターゲットと、素材 `clip` の文字起こしを持つ `project.mmdb` を開いた状態。
/// 本編のコンポジションは、素材の 3〜5 秒を切り出す sequence のレイヤーを持つ。
fn subtitle_state(dir: &std::path::Path) -> Arc<DeliverState> {
    let out = dir.join("out.json");
    let (mut project, comp, _) = vertical_project(&SampleMedia::default(), &out);
    let clip = ParamValue::AssetRef(Id::new("clip"));
    let cut = Cut::new(
        "k1",
        Node::new(FOOTAGE_KIND, 1).with_param(FOOTAGE_PARAM, clip),
        RationalTime::seconds(3),
        RationalTime::seconds(5),
    );
    project.sequences.push(Sequence {
        id: Id::new("s"),
        cuts: vec![cut],
        origin: None,
    });
    let node = Node::new(SEQUENCE_KIND, 1).with_param(SEQUENCE_PARAM, ParamValue::Text("s".into()));
    let seconds = RationalTime::seconds;
    let layer = Layer::new("seq", node, seconds(0), seconds(0), seconds(2));
    project.compositions[0].layers.push(layer);
    project.render_targets[0].delivery.subtitles = SubtitleOutput::Sidecar;
    let store_dir = dir.join("store");
    let mut store = mm_store::Store::create(&store_dir, &project, 0).unwrap();
    let words = [("こんにちは", 3), ("世界", 4)].map(|(text, at)| WordInput {
        segment: 0,
        start: seconds(at),
        end: RationalTime::new(i128::from(at) * 1000 + 800, 1000).unwrap(),
        text: text.into(),
    });
    let input = TranscriptInput {
        producer: "test".into(),
        language: "ja".into(),
        words: words.to_vec(),
    };
    store
        .put_transcript(&Id::new("clip"), &project.assets[0].content_hash, &input)
        .unwrap();
    drop(store);
    let shared = Arc::new(Shared::new());
    shared.publish_doc(Arc::new(project), comp);
    Arc::new(DeliverState::new(shared, &store_dir))
}

#[test]
fn 字幕を出すレンダーターゲットはアプリの書き出しで文字起こしを渡され_字幕のファイルを置く() {
    let dir = temp("subtitles");
    let state = subtitle_state(&dir);
    let (emit, rx) = collect();
    // sequence のレイヤーを描けるのは wgpu のレンダラーだけ（モックは持たない）。共有の GPU は最初に作った
    // バックエンドで固定されるので、再生の試験と同じ D3D12（`present`）を求める。
    let config = RuntimeConfig {
        render: Backend::Wgpu,
        present: true,
        ..RuntimeConfig::sample()
    };
    state.start(request(&["mp4"]), config, emit).unwrap();
    let last = finished(&rx);
    assert_eq!(
        last.items[0].state,
        ItemState::Done,
        "{:?}",
        last.items[0].error
    );
    let srt = std::fs::read_to_string(dir.join("out.srt")).expect("sidecar の字幕");
    assert!(srt.contains("こんにちは"), "{srt}");
    assert!(dir.join("out.vtt").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 一覧の出力先は開くときに直す前の相対のまま返す() {
    let dir = temp("relative");
    let (mut project, comp, _) = vertical_project(&SampleMedia::default(), &dir.join("a.json"));
    project.render_targets[0].output_path = "exports/yt.mp4".into();
    project.render_targets[0].delivery.chapters = true;
    let base = dir.join("proj");
    mm_store::rebase_project(&mut project, &base);
    assert!(Path::new(&project.render_targets[0].output_path).starts_with(&base));
    let shared = Arc::new(Shared::new());
    shared.publish_doc(Arc::new(project), comp);
    let state = DeliverState::new(shared, &base.join("project.json"));
    let overview = state.overview().unwrap();
    assert_eq!(overview.targets[0].output_path, "exports/yt.mp4");
    assert!(
        overview.targets[0].delivery.chapters,
        "ほかのフィールドは変えない"
    );
}

#[test]
fn 大文字小文字と区切りだけが違う出力先も同じとして拒否する() {
    let dir = temp("shared");
    let state = state(&dir);
    let comp = state.shared.doc().unwrap().comp;
    for (a, b) in [("Out.mp4", "out.mp4"), ("a/x.mp4", "a\\x.mp4")] {
        let mut project = (*state.project().unwrap()).clone();
        project.render_targets[0].output_path = a.into();
        project.render_targets[1].output_path = b.into();
        state.shared.publish_doc(Arc::new(project), comp.clone());
        let (emit, _rx) = collect();
        let e = state
            .start(request(&["mp4", "second"]), RuntimeConfig::sample(), emit)
            .unwrap_err();
        assert!(e.contains("同じ出力先"), "{a} と {b}: {e}");
    }
}

#[test]
fn 一時ファイルの名前と重なる出力先も拒否する() {
    let dir = temp("partial");
    let state = state(&dir);
    let comp = state.shared.doc().unwrap().comp;
    let mut project = (*state.project().unwrap()).clone();
    project.render_targets[0].output_path = "x.mp4".into();
    project.render_targets[1].output_path = "x.partial.mp4".into();
    state.shared.publish_doc(Arc::new(project), comp);
    let (emit, _rx) = collect();
    let e = state
        .start(request(&["mp4", "second"]), RuntimeConfig::sample(), emit)
        .unwrap_err();
    assert!(e.contains("同じ出力先"), "{e}");
}

#[test]
fn 一本が失敗しても残りは書き出す() {
    let dir = temp("partial_fail");
    let state = state(&dir);
    let comp = state.shared.doc().unwrap().comp;
    let mut project = (*state.project().unwrap()).clone();
    project.render_targets[0].codec = "no-such-codec".into();
    state.shared.publish_doc(Arc::new(project), comp);
    let (emit, rx) = collect();
    state
        .start(request(&["mp4", "second"]), RuntimeConfig::sample(), emit)
        .unwrap();
    let last = finished(&rx);
    assert_eq!(last.items[0].state, ItemState::Failed);
    assert!(last.items[0].error.is_some());
    assert_eq!(last.items[1].state, ItemState::Done, "{last:?}");
    assert!(dir.join("b.json").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 書き出しのスレッドがパニックしても_キューは止まった状態で知らせる() {
    let dir = temp("panic");
    let state = state(&dir);
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let armed = AtomicBool::new(true);
    let emit: Emit = Arc::new(move |s| {
        let _ = tx.lock().unwrap().send(s.clone());
        let running = s
            .items
            .first()
            .is_some_and(|i| i.state == ItemState::Running);
        if running && armed.swap(false, Ordering::SeqCst) {
            panic!("書き出しのスレッドの途中でパニックする");
        }
    });
    state
        .start(request(&["mp4", "second"]), RuntimeConfig::sample(), emit)
        .unwrap();
    let last = finished(&rx);
    assert!(
        last.items
            .iter()
            .all(|i| i.state == ItemState::Failed && i.error.is_some()),
        "{last:?}"
    );
    assert!(!state.status().running);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn キューを止めると_動いていない項目だけ理由つきの失敗になる() {
    let target = state(&temp("stop")).project().unwrap().render_targets[0].clone();
    let queued = ExportItem::queued(&target);
    let done = ExportItem {
        state: ItemState::Done,
        ..queued.clone()
    };
    let status = Mutex::new(ExportStatus {
        running: true,
        items: vec![done, queued],
    });
    let stopped = stop_all(&status, "スレッドを作れない");
    assert!(!stopped.running);
    assert_eq!(stopped.items[0].state, ItemState::Done);
    assert_eq!(stopped.items[1].state, ItemState::Failed);
    assert_eq!(
        stopped.items[1].error.as_deref(),
        Some("スレッドを作れない")
    );
    assert_eq!(*status.lock().unwrap(), stopped);
}

#[test]
fn 進み具合は_total_が_0_のうちは終わりにせず_最後のフレームは間隔を待たずに送る() {
    let (emit, rx) = collect();
    let target = state(&temp("progress")).project().unwrap().render_targets[0].clone();
    let status = Arc::new(Mutex::new(ExportStatus {
        running: true,
        items: vec![ExportItem::queued(&target)],
    }));
    let progress = progress_reporter(status, emit, 0);
    let at = |done, total| ExportProgress {
        target: target.id.clone(),
        frames_done: done,
        frames_total: total,
        elapsed: Duration::from_millis(1),
    };
    progress(&at(0, 0));
    assert!(rx.try_recv().is_err(), "合計が 0 のうちは間隔を待つ");
    progress(&at(1, 1));
    assert_eq!(rx.try_recv().unwrap().items[0].frames_done, 1);
}

#[test]
fn 保存ダイアログの拡張子はコーデックに合う() {
    assert_eq!(output_extensions("prores_ks"), ["mov"]);
    assert_eq!(output_extensions("dnxhr_hq"), ["mov"]);
    assert_eq!(output_extensions("h264_nvenc"), ["mp4", "mov"]);
}

#[test]
fn 書き出しのレンダラーは再生より小さい_vram_のキャッシュを持つ() {
    let config = export_config();
    assert_eq!(config.frame_memory_bytes, Some(EXPORT_FRAME_MEMORY_BYTES));
    assert!(!config.present);
}

#[test]
fn ffprobe_を起動できなければ検証を使えないとする() {
    std::env::set_var("MM_FFPROBE", temp("none").join("ffprobe-missing"));
    let available = ffprobe_available();
    std::env::remove_var("MM_FFPROBE");
    assert!(!available);
}
