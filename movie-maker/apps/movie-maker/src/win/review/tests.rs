use mm_doc::Topic;
use mm_ops::{Entry, Op, Transaction};
use mm_serve::Caller;
use mm_store::{Actor, Store};
use mm_types::{Id, RationalTime};

use super::*;
use crate::win::sequence_edit::tests::sample_project;

/// `project.mmdb` を書き手として開いたアプリと同じ共有状態。再生スレッドは無い。
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

fn mcp() -> Caller {
    Caller {
        entry: Entry::Mcp,
        actor: Actor {
            kind: "mcp".into(),
            name: "claude".into(),
        },
    }
}

fn propose(core: &Core, topic_id: &str) -> i64 {
    let txn = Transaction::new("話題を足す", vec![topic(topic_id)]);
    core.propose(txn, &mcp(), None).expect("提案を出す").id
}

fn preview(id: i64) -> Preview {
    Preview { id, ops: None }
}

#[test]
fn 適用できない提案に切り替えると今のプレビューのまま失敗を返す() {
    let dir = tempfile::tempdir().unwrap();
    let (shared, core) = saved(dir.path());
    let id = propose(&core, "t1");
    set_preview(&shared, Some(preview(id))).expect("適用する");
    assert!(set_preview(&shared, Some(preview(id + 100))).is_err());
    assert_eq!(shared.review_preview.get(), Some(preview(id)));
}

#[test]
fn 承認した提案は適用し直さず元のフレームに戻す() {
    let dir = tempfile::tempdir().unwrap();
    let (shared, core) = saved(dir.path());
    let id = propose(&core, "t1");
    set_preview(&shared, Some(preview(id))).expect("適用する");
    core.approve(id, None, &Caller::webview())
        .expect("承認する");
    let mut touched = Vec::new();
    assert!(
        previewed(&shared, &mut touched).is_none(),
        "承認済みを二重に適用しない"
    );
    assert_eq!(shared.review_preview.get(), None);
    assert!(
        touched.iter().any(|t| t == "t1"),
        "外したプレビューが触った所のキャッシュも捨てる: {touched:?}"
    );
}

#[test]
fn 外したプレビューを当て直しの結果で書き戻さない() {
    let slot = PreviewSlot::default();
    slot.set(preview(1), vec!["a".into()]);
    let stale = preview(1);
    slot.take();
    assert!(!slot.refresh(&stale, vec!["b".into()]));
    assert_eq!(slot.get(), None);
    slot.set(preview(2), Vec::new());
    assert!(slot.drop_if(&stale).is_empty());
    assert_eq!(
        slot.get(),
        Some(preview(2)),
        "別の提案のプレビューは外さない"
    );
}

#[test]
fn 承認の取り消しは一番上のときだけ戻す() {
    let dir = tempfile::tempdir().unwrap();
    let (shared, core) = saved(dir.path());
    let id = propose(&core, "t1");
    core.approve(id, None, &Caller::webview())
        .expect("承認する");
    crate::win::edit::submit(&shared, vec![topic("t2")]).expect("後から編集する");
    assert!(undo_approved(&shared, &core, id).is_err());
    crate::win::edit::step_saved(&shared, &core, true);
    let undone = undo_approved(&shared, &core, id).expect("一番上になったので戻せる");
    assert!(!undone.applied);
}
