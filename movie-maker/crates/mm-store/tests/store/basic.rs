//! 開く、書く、取り消す、読み取りだけで開く、バックアップ、検索、同期のフォルダー。

use std::fs;

use mm_ops::{Editor, OpContext, Transaction, TxnKind};
use serde_json::json;

use mm_store::{OpenMode, Store, StoreWarning, SyncKind};
use mm_types::MmError;

use crate::common::{actor, locked, project_with, toggle_lock, NOW};

#[test]
fn 書いたプロジェクトのデータと操作は開き直しても残る() {
    let tmp = tempfile::tempdir().unwrap();
    let mut p = project_with(2);
    {
        let mut s = Store::create(tmp.path(), &p, NOW).unwrap();
        let input = locked(&mut p, "t1", &[0, 1]);
        let t = s.commit(&p, &input).unwrap();
        assert_eq!((t.id, t.first_seq, t.last_seq, t.doc_version), (1, 1, 2, 2));
        assert_eq!(t.kind, TxnKind::Normal);
        s.close(NOW).unwrap();
    }
    let s = Store::open(tmp.path(), NOW).unwrap();
    assert_eq!(s.project(), &p);
    assert_eq!(s.doc_version(), 2);
    let txns = s.txns_between(0, 2).unwrap();
    assert_eq!(txns[0].ops[1].record.inverse["layer"], "l1");
    assert_eq!(txns[0].ops[0].name, "lockLayer");
    assert_eq!(txns[0].record().op_version(1), 2);
}

#[test]
fn 取り消しとやり直しは位置を動かし_バージョンと記録を進める() {
    let tmp = tempfile::tempdir().unwrap();
    let mut p = project_with(3);
    let mut s = Store::create(tmp.path(), &p, NOW).unwrap();
    let p0 = p.clone();
    let input = locked(&mut p, "t1", &[0]);
    s.commit(&p, &input).unwrap();
    let p1 = p.clone();
    let input = locked(&mut p, "t2", &[1]);
    s.commit(&p, &input).unwrap();

    let human = OpContext::human();
    let undo = s.undo_last(&human, actor(), NOW).unwrap().unwrap();
    assert_eq!(undo.kind, TxnKind::Undo { of: 2 });
    assert_eq!((undo.id, undo.base_version, undo.doc_version), (3, 2, 3));
    assert_eq!(undo.name, "取り消し: t2");
    assert_eq!(s.project(), &p1);
    assert_eq!(s.undo_depth(), 1);
    assert_eq!(s.redo_plan().unwrap().unwrap().txn_id, 2);

    let redo = s.redo_last(&human, actor(), NOW).unwrap().unwrap();
    assert_eq!(redo.kind, TxnKind::Redo { of: 2 });
    assert_eq!(redo.doc_version, 4);
    assert_eq!(s.project(), &p);

    // 取り消した後の新しい操作で、やり直しの先が破棄される。
    s.undo_last(&human, actor(), NOW).unwrap().unwrap();
    s.undo_last(&human, actor(), NOW).unwrap().unwrap();
    assert_eq!(s.project(), &p0);
    assert!(s.undo_last(&human, actor(), NOW).unwrap().is_none());
    let mut q = p0.clone();
    let input = locked(&mut q, "t3", &[2]);
    s.commit(&q, &input).unwrap();
    assert!(s.redo_plan().unwrap().is_none());
    assert!(s.redo_last(&human, actor(), NOW).unwrap().is_none());
    assert_eq!(s.undo_depth(), 1);
    assert_eq!(s.doc_version(), 7);
    drop(s);
    let s = Store::open(tmp.path(), NOW).unwrap();
    assert_eq!(s.undo_depth(), 1);
    assert_eq!(s.undo_plan().unwrap().unwrap().txn_id, 7);
}

#[test]
fn 列の位置に合わない取り消しの記録は拒否する() {
    let tmp = tempfile::tempdir().unwrap();
    let mut p = project_with(2);
    let mut s = Store::create(tmp.path(), &p, NOW).unwrap();
    let input = locked(&mut p, "t1", &[0]);
    s.commit(&p, &input).unwrap();
    let mut input = locked(&mut p, "勝手な取り消し", &[1]);
    input.kind = TxnKind::Undo { of: 9 };
    assert!(matches!(
        s.commit(&p, &input).unwrap_err(),
        MmError::InvalidArgument(_)
    ));
    input.kind = TxnKind::Redo { of: 1 };
    assert!(s.commit(&p, &input).is_err());
    assert_eq!(s.doc_version(), 1);
}

#[test]
fn editor_と同じ_id_とバージョンの数え方になる() {
    /// 同じ操作を両方に適用して、プロジェクトのデータ、バージョン、記録が一致することを見る。
    enum Step {
        Apply(Vec<usize>),
        Undo,
        Redo,
    }
    let tmp = tempfile::tempdir().unwrap();
    let p = project_with(3);
    let mut s = Store::create(tmp.path(), &p, NOW).unwrap();
    let mut ed = Editor::new(p);
    let human = OpContext::human();
    let steps = [
        Step::Apply(vec![0, 1]),
        Step::Undo,
        Step::Redo,
        Step::Undo,
        Step::Apply(vec![2]),
        Step::Undo,
        Step::Redo,
    ];
    for step in steps {
        match step {
            Step::Apply(ids) => {
                let ops = ids.iter().map(|&i| toggle_lock(s.project(), i)).collect();
                let t = Transaction::new("t", ops);
                s.apply_txn(t.clone(), &human, actor(), NOW, None).unwrap();
                ed.apply(t, &human).unwrap();
            }
            Step::Undo => {
                s.undo_last(&human, actor(), NOW).unwrap().unwrap();
                ed.undo(&human).unwrap().unwrap();
            }
            Step::Redo => {
                s.redo_last(&human, actor(), NOW).unwrap().unwrap();
                ed.redo(&human).unwrap().unwrap();
            }
        }
        assert_eq!(s.project(), ed.project());
        assert_eq!(s.doc_version() as u64, ed.version());
        let stored: Vec<_> = s.txn_log().unwrap().iter().map(|t| t.record()).collect();
        assert_eq!(stored, ed.log());
    }
    assert_eq!(s.doc_version(), 11);
}

#[test]
fn 別のプロセスが書き手なら読み取りだけで開く() {
    let tmp = tempfile::tempdir().unwrap();
    let mut p = project_with(2);
    let mut writer = Store::create(tmp.path(), &p, NOW).unwrap();
    writer.publish_http_port(4321).unwrap();
    let input = locked(&mut p, "t1", &[0]);
    writer.commit(&p, &input).unwrap();

    let mut reader = Store::open(tmp.path(), NOW).unwrap();
    assert_eq!(reader.mode(), OpenMode::ReadOnly);
    assert_eq!(reader.project(), &p);
    let StoreWarning::WriterElsewhere(Some(info)) = reader.warnings().last().unwrap() else {
        panic!("{:?}", reader.warnings());
    };
    assert_eq!(info.http_port, Some(4321));
    let err = reader
        .commit(&p, &locked(&mut p.clone(), "t2", &[1]))
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
fn 読むだけで開くとロックを取らず_書き手を締め出さない() {
    let tmp = tempfile::tempdir().unwrap();
    let mut p = project_with(2);
    drop(Store::create(tmp.path(), &p, NOW).unwrap());

    let reader = Store::open_read_only(tmp.path()).unwrap();
    assert_eq!(reader.mode(), OpenMode::ReadOnly);
    assert!(reader.warnings().is_empty(), "{:?}", reader.warnings());
    let mut writer = Store::open(tmp.path(), NOW).unwrap();
    assert_eq!(writer.mode(), OpenMode::Writer, "読む側はロックを持たない");
    let input = locked(&mut p, "t1", &[0]);
    writer.commit(&p, &input).unwrap();

    let reader = Store::open_read_only(tmp.path()).unwrap();
    assert_eq!(
        reader.project(),
        &p,
        "書き手がいる間も今のプロジェクトのデータを読める"
    );
    assert!(matches!(
        reader.warnings(),
        [StoreWarning::WriterElsewhere(Some(_))]
    ));
    assert!(
        reader.export_versions().is_ok(),
        "書き出しは読むだけでできる"
    );
}

#[test]
fn バックアップから開ける() {
    let tmp = tempfile::tempdir().unwrap();
    let mut p = project_with(2);
    let mut s = Store::create(tmp.path(), &p, NOW).unwrap();
    let input = locked(&mut p, "t1", &[0]);
    s.commit(&p, &input).unwrap();
    let p1 = p.clone();
    let path = s.backup(NOW).unwrap();
    let input = locked(&mut p, "t2", &[1]);
    s.commit(&p, &input).unwrap();
    drop(s);

    let copy = tempfile::tempdir().unwrap();
    fs::copy(&path, copy.path().join("project.mmdb")).unwrap();
    let restored = Store::open(copy.path(), NOW).unwrap();
    assert_eq!(restored.project(), &p1);
    assert!(
        !fs::read_dir(tmp.path().join("backups")).unwrap().any(|e| e
            .unwrap()
            .path()
            .extension()
            .is_some_and(|x| x == "tmp")),
        "書きかけのコピーは残らない"
    );
}

#[test]
fn 間隔が過ぎたらバックアップを作り_前回の時刻はプロセスをまたいで数える() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::create(tmp.path(), &project_with(0), NOW).unwrap();
    assert!(s.backup_if_due(NOW + 14 * 60_000).unwrap().is_none());
    assert!(s.backup_if_due(NOW + 15 * 60_000).unwrap().is_some());
    drop(s);

    let mut s = Store::open(tmp.path(), NOW + 20 * 60_000).unwrap();
    assert!(
        s.backup_if_due(NOW + 29 * 60_000).unwrap().is_none(),
        "開き直しても前回（15 分の時点）から数える"
    );
    assert!(s.backup_if_due(NOW + 30 * 60_000).unwrap().is_some());
    s.close_brief(NOW + 31 * 60_000).unwrap();
    assert_eq!(
        fs::read_dir(tmp.path().join("backups")).unwrap().count(),
        2,
        "close_brief は間隔の内なら写さない"
    );
}

#[test]
fn 壊れた_db_は戻し先つきで拒否する() {
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
fn 作る途中のファイルは_project_mmdb_の名前にならない() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("project.mmdb.new"), b"half").unwrap();
    assert!(Store::open(tmp.path(), NOW).is_err());
    let s = Store::create(tmp.path(), &project_with(1), NOW).unwrap();
    assert_eq!(s.doc_version(), 0);
    assert!(!tmp.path().join("project.mmdb.new").exists());
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
    let mut p = s.project().clone();
    let input = locked(&mut p, "t1", &[0]);
    p.compositions[0].layers.pop();
    s.commit(&p, &input).unwrap();
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
    s.put_comment(
        &json!({ "id": "r1", "state": "open", "comp_id": "c", "text": "アウト点を詰める" }),
    )
    .unwrap();
    assert_eq!(s.comments().unwrap()[0]["text"], "アウト点を詰める");
}
