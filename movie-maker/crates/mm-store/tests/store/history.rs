//! 取り消しとやり直しが記録と競合の判定に見えること、壊れた DB から `import` で戻れること。

use std::fs;

use mm_ops::{LockLayer, Op, OpContext, OpError, Transaction};
use mm_store::Store;
use mm_types::LayerId;

use crate::common::{actor, project_with, NOW};

fn lock(i: usize, lock: bool) -> Op {
    LockLayer {
        layer: LayerId::new(format!("l{i}")),
        lock,
    }
    .into()
}

fn apply(store: &mut Store, name: &str, ops: Vec<Op>, base: Option<u64>) -> mm_ops::OpResult<i64> {
    let mut txn = Transaction::new(name, ops);
    txn.base_version = base;
    store
        .apply_txn(txn, &OpContext::human(), actor(), NOW, None)
        .map(|t| t.doc_version)
}

#[test]
fn 取り消しの後も古い版を元にした命令は競合で断られる() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::create(tmp.path(), &project_with(2), NOW).unwrap();
    assert_eq!(
        apply(&mut s, "ロック", vec![lock(0, true)], None).unwrap(),
        1
    );
    s.undo_last(&OpContext::human(), actor(), NOW)
        .unwrap()
        .unwrap();
    assert_eq!(s.doc_version(), 2, "取り消しも版を進める");

    // 版 1 を元にした l0 への命令は、版 2 の取り消しと触る所が重なる。
    let err = apply(&mut s, "古い", vec![lock(0, true)], Some(1)).unwrap_err();
    assert!(matches!(err, OpError::Conflict { .. }), "{err:?}");

    s.redo_last(&OpContext::human(), actor(), NOW)
        .unwrap()
        .unwrap();
    assert_eq!(s.doc_version(), 3);
    let err = apply(&mut s, "古い", vec![lock(0, false)], Some(2)).unwrap_err();
    assert!(matches!(err, OpError::Conflict { .. }), "{err:?}");
    // l1 は誰も触っていない。
    assert_eq!(
        apply(&mut s, "別", vec![lock(1, true)], Some(1)).unwrap(),
        4
    );
}

#[test]
fn 当てた版は取り消しても変わらず_diff_は取り消しと対で畳む() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::create(tmp.path(), &project_with(2), NOW).unwrap();
    apply(&mut s, "A", vec![lock(0, true)], None).unwrap();
    s.undo_last(&OpContext::human(), actor(), NOW)
        .unwrap()
        .unwrap();

    assert_eq!(
        s.txns_between(0, 1).unwrap().len(),
        1,
        "版 1 には A が当たっている"
    );
    assert!(
        s.txns_between(0, 2).unwrap().is_empty(),
        "A とその取り消しは畳む"
    );
    let undo_only = s.txns_between(1, 2).unwrap();
    assert_eq!(undo_only.len(), 1);
    assert_eq!(
        undo_only[0].ops[0].record.op["lock"], false,
        "取り消しの行は逆命令"
    );

    s.redo_last(&OpContext::human(), actor(), NOW)
        .unwrap()
        .unwrap();
    assert_eq!(
        s.txns_between(0, 3).unwrap().len(),
        1,
        "やり直しは取り消しと畳んで A が残る"
    );
    assert_eq!(
        s.txns_between(2, 3).unwrap()[0].ops[0].record.op["lock"],
        true
    );
    assert_eq!(s.doc_version_at(NOW).unwrap(), 3);
    let log = s.txn_log().unwrap();
    assert_eq!(log.len(), 3);
    assert_eq!(log[0].doc_version, 1, "A の版は書き換えない");
}

#[test]
fn 壊れた_db_でも_import_の置き換えで戻れる() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let mut s = Store::create(tmp.path(), &project_with(2), NOW).unwrap();
        apply(&mut s, "A", vec![lock(0, true)], None).unwrap();
        s.export_versions().unwrap();
        s.close(NOW).unwrap();
    }
    let db = tmp.path().join("project.mmdb");
    let mut bytes = fs::read(&db).unwrap();
    for b in bytes.iter_mut().skip(100) {
        *b = 0xA5;
    }
    fs::write(&db, bytes).unwrap();
    assert!(Store::open(tmp.path(), NOW).is_err());

    let versions = tmp.path().join("versions");
    let restored = Store::import_versions(tmp.path(), &versions, true, NOW + 1000).unwrap();
    assert!(restored.project().compositions[0].layers[0].lock);
    let kept: Vec<String> = fs::read_dir(tmp.path().join("backups"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        kept.iter().any(|n| n.contains("broken")),
        "壊れたファイルは backups/ に残す: {kept:?}"
    );
}
