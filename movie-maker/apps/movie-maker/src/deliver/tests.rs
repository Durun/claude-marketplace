use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use mm_testkit::decode::SampleMedia;
use mm_testkit::fixtures::vertical_project;

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
    Arc::new(DeliverState::new(shared))
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
fn 列のレンダーターゲットを順に書き出し_書き出したファイルだけフォルダーを開ける() {
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
fn 同じ出力先を指す組と_無い対象は始める前に断る() {
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
    assert!(!state.status().running, "断ったときは列を作らない");
}

#[test]
fn 取り消すと走っている分を止め_残りは書き出さず_一時ファイルを残さない() {
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
fn 書き出し中にもう一度始めようとすると断る() {
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
