use std::sync::mpsc::channel;

use mm_doc::{Asset, Project, Topic};
use mm_serve::{Caller, Core};
use mm_store::{Actor, Store};
use mm_types::{ColorSpace, Id, RationalTime};

use super::*;
use crate::win::sequence_edit::tests::sample_project;
use crate::win::sequence_edit::{plan_insert, Ctx, SourceRange};

fn op() -> Op {
    mm_ops::SetParam {
        target: mm_ops::ParamRef::layer(&mm_types::Id::new("l"), "opacity"),
        value: mm_types::ParamValue::Number(0.5),
        keyframes: None,
    }
    .into()
}

#[test]
fn 時間切れで返した操作は後から適用されない() {
    let shared = Shared::new();
    let r = submit_within(&shared, vec![op()], Duration::from_millis(20));
    assert!(r.is_err());
    let late = shared.edits.lock().unwrap().pop().expect("積まれたまま");
    assert!(!late.claim(), "取り下げた操作は適用する権利を取れない");
}

#[test]
fn 適用し始めた操作は時間を超えても結果を待つ() {
    let shared = Arc::new(Shared::new());
    let s = shared.clone();
    let player = std::thread::spawn(move || {
        let edit = loop {
            if let Some(e) = s.edits.lock().unwrap().pop() {
                break e;
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        assert!(edit.claim());
        std::thread::sleep(Duration::from_millis(80));
        let _ = edit.reply.send(Ok(EditOutcome {
            touched: vec!["l".into()],
        }));
    });
    let r = submit_within(&shared, vec![op()], Duration::from_millis(30));
    player.join().unwrap();
    assert_eq!(r.expect("適用した結果を返す").touched, ["l"]);
}

/// `project.mmdb` を書き手として開いたアプリと同じ共有状態。再生スレッドは無い（データの置き直しを待たない）。
fn saved(dir: &std::path::Path) -> (Arc<Shared>, Arc<Core>) {
    let project = sample_project();
    let store = Store::create(dir, &project, 0).expect("project.mmdb を作る");
    let core = Core::new(store, None);
    let shared = Arc::new(Shared::new());
    let _ = shared.core.set(core.clone());
    let version = core.version().unwrap();
    shared.publish_saved(version, Arc::new(project), Id::new("main"));
    (shared, core)
}

fn topic(id: &str) -> Op {
    mm_ops::AddTopic {
        topic: Topic {
            id: Id::new(id),
            asset: Id::new("a"),
            in_point: RationalTime::seconds(1),
            out_point: RationalTime::seconds(2),
            title: "話題".into(),
            rating: Default::default(),
            tags: Vec::new(),
        },
    }
    .into()
}

fn cli() -> Caller {
    Caller {
        entry: mm_ops::Entry::Cli,
        actor: Actor {
            kind: "cli".into(),
            name: "mm".into(),
        },
    }
}

fn doc_project(shared: &Shared) -> Arc<Project> {
    shared.doc().expect("開いている").project
}

#[test]
fn 話題を作った直後に読み直すと作った話題が出る() {
    let dir = tempfile::tempdir().unwrap();
    let (shared, _core) = saved(dir.path());
    submit(&shared, vec![topic("t1")]).expect("話題を作る");
    // Story の reload は apply_op が返った直後に Shared::doc を読む。
    assert!(doc_project(&shared).topic(&Id::new("t1")).is_ok());
}

#[test]
fn 取り込んだ直後に読み直すと素材の一覧に出る() {
    let dir = tempfile::tempdir().unwrap();
    let (shared, _core) = saved(dir.path());
    let asset = Asset {
        id: Id::new("b"),
        path: "b.mp4".into(),
        content_hash: "ab12".into(),
        color_space: ColorSpace::bt709(),
        duration: RationalTime::seconds(3),
        fps: None,
        timebase_offset: RationalTime::ZERO,
    };
    let add = mm_ops::AddAsset { asset, index: None };
    submit(&shared, vec![add.into()]).expect("素材を足す");
    let media = crate::win::media::MediaState::new(shared.clone(), dir.path().join("cache"));
    assert!(media.list().iter().any(|m| m.id == "b"));
}

#[test]
fn cli_の操作は画面に知らせる前にデータへ入る() {
    let dir = tempfile::tempdir().unwrap();
    let (shared, core) = saved(dir.path());
    let (tx, rx) = channel();
    let s = shared.clone();
    watch_changes(&shared, move |_| {
        let seen = doc_project(&s).topic(&Id::new("t-cli")).is_ok();
        let _ = tx.send(seen);
    })
    .expect("通知を受ける");
    let txn = mm_ops::Transaction::new("mm topic add", vec![topic("t-cli")]);
    core.apply(txn, &cli(), None).expect("CLI の操作");
    let seen = rx.recv_timeout(Duration::from_secs(5)).expect("通知が届く");
    assert!(seen, "通知を受けた側が読み直すと CLI の話題が出る");
}

fn insert_range() -> SourceRange {
    SourceRange {
        asset: Id::new("a"),
        in_point: RationalTime::seconds(5),
        out_point: RationalTime::seconds(6),
    }
}

#[test]
fn cli_が_extract_した後に古いデータで計画した_insert_は衝突で断られる() {
    let dir = tempfile::tempdir().unwrap();
    let (shared, core) = saved(dir.path());
    let doc = shared.doc().unwrap();
    let ctx = Ctx {
        project: &doc.project,
        comp: doc.project.composition(&doc.comp).unwrap(),
        layer: None,
        cut: None,
        playhead: 3.2,
    };
    let planned = plan_insert(&ctx, &insert_range(), Id::new("k-new")).unwrap();
    let extract = mm_ops::Extract { cut: Id::new("k2") };
    let txn = mm_ops::Transaction::new("mm extract", vec![extract.into()]);
    core.apply(txn, &cli(), None).expect("CLI の extract");

    let refused = submit_based(&shared, vec![planned.op], doc.version);
    let error = refused.expect_err("古い編集点への insert は適用しない");
    assert!(error.contains("触る所が重なる"), "衝突として断る: {error}");
    let seq = &core.project().unwrap().project.sequences[0];
    assert!(seq.cuts.iter().all(|c| c.id.as_str() != "k-new"));
}

#[test]
fn 編集の列は積んだ順に_1_つずつ回す() {
    let shared = Arc::new(Shared::new());
    let (tx, rx) = channel();
    for n in 0..5 {
        let tx = tx.clone();
        queue(&shared, move || {
            std::thread::sleep(Duration::from_millis(5 - n));
            let _ = tx.send(n);
        });
    }
    let order: Vec<u64> = (0..5)
        .map(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap())
        .collect();
    assert_eq!(order, [0, 1, 2, 3, 4]);
}
