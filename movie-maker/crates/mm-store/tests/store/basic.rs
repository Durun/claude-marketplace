//! 開く、書く、取り消す、読み取りだけで開く、バックアップ、検索、同期のフォルダー。

use std::fs;

use serde_json::json;

use mm_store::{OpenMode, Store, StoreWarning, SyncKind};
use mm_types::MmError;

use crate::common::{project_with, txn, NOW};

#[test]
fn 書いた文書と命令は開き直しても残る() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let mut s = Store::create(tmp.path(), &project_with(0), NOW).unwrap();
        let info = s.commit(&project_with(2), &txn("t1", &[0, 1])).unwrap();
        assert_eq!((info.first_seq, info.last_seq, info.doc_version), (1, 2, 2));
        s.close(NOW).unwrap();
    }
    let s = Store::open(tmp.path(), NOW).unwrap();
    assert_eq!(s.project(), &project_with(2));
    assert_eq!(s.doc_version(), 2);
    let txns = s.txns_between(0, 2).unwrap();
    assert_eq!(txns[0].ops[1].record.inverse["id"], "l1");
    assert_eq!(txns[0].ops[0].name, "addLayer");
}

#[test]
fn 取り消しとやり直しは位置と状態を動かす() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::create(tmp.path(), &project_with(0), NOW).unwrap();
    s.commit(&project_with(1), &txn("t1", &[0])).unwrap();
    s.commit(&project_with(2), &txn("t2", &[1])).unwrap();

    let plan = s.undo_plan().unwrap().unwrap();
    assert_eq!(plan.txn_id, "t2");
    s.commit_undo(&project_with(1)).unwrap();
    assert_eq!(s.project(), &project_with(1));
    assert_eq!(s.doc_version(), 3);
    assert_eq!(s.redo_plan().unwrap().unwrap().txn_id, "t2");
    // 取り消されたトランザクションは diff に出ない。
    assert_eq!(s.txns_between(0, 3).unwrap().len(), 1);

    s.commit_redo(&project_with(2)).unwrap();
    assert_eq!(s.txns_between(0, 4).unwrap().len(), 2);

    // 取り消した後の新しい命令で、やり直しの先が捨てられる。
    s.commit_undo(&project_with(1)).unwrap();
    s.commit(&project_with(1), &txn("t3", &[5])).unwrap();
    assert!(s.redo_plan().unwrap().is_none());
    assert_eq!(s.undo_depth(), 2);
    drop(s);
    let s = Store::open(tmp.path(), NOW).unwrap();
    assert_eq!(s.undo_depth(), 2);
    assert_eq!(s.undo_plan().unwrap().unwrap().txn_id, "t3");
}

#[test]
fn 別のプロセスが書き手なら読み取りだけで開く() {
    let tmp = tempfile::tempdir().unwrap();
    let mut writer = Store::create(tmp.path(), &project_with(0), NOW).unwrap();
    writer.publish_http_port(4321).unwrap();
    writer.commit(&project_with(1), &txn("t1", &[0])).unwrap();

    let mut reader = Store::open(tmp.path(), NOW).unwrap();
    assert_eq!(reader.mode(), OpenMode::ReadOnly);
    assert_eq!(reader.project(), &project_with(1));
    let StoreWarning::WriterElsewhere(Some(info)) = reader.warnings().last().unwrap() else {
        panic!("{:?}", reader.warnings());
    };
    assert_eq!(info.http_port, Some(4321));
    let err = reader
        .commit(&project_with(2), &txn("t2", &[1]))
        .unwrap_err();
    assert!(matches!(err, MmError::Unsupported(_)));

    drop(reader);
    drop(writer);
    assert_eq!(
        Store::open(tmp.path(), NOW).unwrap().mode(),
        OpenMode::Writer
    );
}

#[test]
fn バックアップから開ける() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::create(tmp.path(), &project_with(0), NOW).unwrap();
    s.commit(&project_with(1), &txn("t1", &[0])).unwrap();
    let path = s.backup(NOW).unwrap();
    s.commit(&project_with(2), &txn("t2", &[1])).unwrap();
    drop(s);

    let copy = tempfile::tempdir().unwrap();
    fs::copy(&path, copy.path().join("project.mmdb")).unwrap();
    let restored = Store::open(copy.path(), NOW).unwrap();
    assert_eq!(restored.project(), &project_with(1));
}

#[test]
fn 間隔が過ぎたらバックアップを作る() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::create(tmp.path(), &project_with(0), NOW).unwrap();
    assert!(s.backup_if_due(NOW + 14 * 60_000).unwrap().is_none());
    assert!(s.backup_if_due(NOW + 15 * 60_000).unwrap().is_some());
}

#[test]
fn 壊れた_db_は戻し先つきで断る() {
    let tmp = tempfile::tempdir().unwrap();
    drop(Store::create(tmp.path(), &project_with(1), NOW).unwrap());
    let db = tmp.path().join("project.mmdb");
    let mut bytes = fs::read(&db).unwrap();
    bytes.truncate(bytes.len() / 2);
    for b in bytes.iter_mut().skip(100) {
        *b = 0xA5;
    }
    fs::write(&db, bytes).unwrap();
    let _ = fs::remove_file(tmp.path().join("project.mmdb-wal"));
    let err = Store::open(tmp.path(), NOW).unwrap_err();
    assert!(matches!(err, MmError::Corrupt(_)), "{err:?}");
}

#[test]
fn レイヤー名とマーカー名を引ける() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::create(tmp.path(), &project_with(2), NOW).unwrap();
    let hits = s.search("オープニング", 10).unwrap();
    assert_eq!(
        (hits[0].kind.as_str(), hits[0].text.as_str()),
        ("marker", "オープニングの章")
    );
    let hits = s.search("字幕その1", 10).unwrap();
    assert_eq!(hits[0].owner, "layers/c/l1");
    assert_eq!(s.search("字", 10).unwrap().len(), 2, "3 文字未満は部分一致");

    // レイヤーを減らすと検索からも消える。
    s.commit(&project_with(1), &txn("t1", &[0])).unwrap();
    assert!(s.search("字幕その1", 10).unwrap().is_empty());
}

#[test]
fn 同期のフォルダーでは警告して間隔を縮める() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Dropbox").join("proj");
    let s = Store::create(&dir, &project_with(0), NOW).unwrap();
    assert!(matches!(
        s.warnings(),
        [StoreWarning::SyncFolder(f)] if f.kind == SyncKind::Dropbox
    ));
    assert_eq!(s.backup_interval(), 5 * 60_000);
    s.close(NOW).unwrap();
    assert!(dir.join("versions").join("project.json").exists());
}

#[test]
fn コメントは保存される() {
    let tmp = tempfile::tempdir().unwrap();
    let s = Store::create(tmp.path(), &project_with(0), NOW).unwrap();
    s.put_comment(&json!({ "id": "r1", "state": "open", "comp_id": "c", "text": "尻を詰める" }))
        .unwrap();
    assert_eq!(s.comments().unwrap()[0]["text"], "尻を詰める");
}
