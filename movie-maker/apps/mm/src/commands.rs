//! プロジェクトのフォルダー（`project.mmdb`）に対する `mm` のサブコマンド。
//! 書く操作は書き手のロックを取れたときだけ受け、別のプロセスが書いているときは拒否する。
//! 読むだけのコマンド（`log`、`diff`、`export`）はロックを取らずに開き、アプリの邪魔をしない。

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use mm_doc::Project;
use mm_ops::{Entry, OpContext, OpError, Transaction};
use mm_store::{format_timestamp, Actor, OpenMode, Store, StoreWarning, TxnKind};
use mm_types::{MmError, Result};

use crate::describe::describe_txns;
use crate::opargs::{parse_op, usage};
use crate::project::load_json_absolute;

/// 記録に残す操作経路の種類と名前。
const ACTOR_KIND: &str = "cli";
const ACTOR_NAME: &str = "mm";

/// 今の時刻（UNIX のミリ秒）。
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn actor() -> Actor {
    Actor {
        kind: ACTOR_KIND.into(),
        name: ACTOR_NAME.into(),
    }
}

/// CLI は Claude の操作経路として、ロックの付いた所に触る操作を拒否する。
fn cli_context() -> OpContext {
    OpContext::new(Entry::Cli)
}

/// 操作の層の拒否を、`mm` のエラーにする。
fn op_error(e: OpError) -> MmError {
    match e {
        OpError::Doc(inner) => inner,
        other => MmError::InvalidArgument(format!("操作を拒否した: {other}")),
    }
}

/// 書き手として開く。別のプロセスが書き手なら、知らせて拒否する。
fn open_writer(dir: &Path) -> Result<Store> {
    let store = Store::open(dir, now_ms())?;
    if store.mode() == OpenMode::Writer {
        return Ok(store);
    }
    Err(MmError::InvalidArgument(writer_elsewhere(&store)))
}

fn writer_elsewhere(store: &Store) -> String {
    let info = store.warnings().iter().find_map(|w| match w {
        StoreWarning::WriterElsewhere(info) => info.as_ref(),
        _ => None,
    });
    let who = match info {
        Some(i) => match i.http_port {
            Some(port) => format!("pid {}、HTTP ポート {port}", i.pid),
            None => format!("pid {}", i.pid),
        },
        None => "pid 不明".to_owned(),
    };
    format!(
        "別のプロセスが書いている（{who}）ので書けない。読み取りだけできる（mm log、mm diff、mm export）。\
         アプリへの HTTP での転送は mm-serve まで無い"
    )
}

/// 読むだけの用途で開く。ロックを取らず、別のプロセスが書き手でもそのまま読む。
fn open_reader(dir: &Path) -> Result<Store> {
    Store::open_read_only(dir)
}

/// 書き終えて閉じる。WAL の書き戻しと、間隔が過ぎていればバックアップ。
fn finish(store: Store) -> Result<()> {
    store.close_brief(now_ms())
}

/// `mm new`: 空のプロジェクトを作る。
pub fn new_project(dir: &Path) -> Result<()> {
    let store = Store::create(dir, &Project::new(), now_ms())?;
    println!("{}: 空のプロジェクトを作った", dir.display());
    finish(store)
}

/// `mm import`: `versions/` のフォルダーか、1 つの JSON から `project.mmdb` を作る。
pub fn import(dir: &Path, source: &Path, overwrite: bool) -> Result<()> {
    let store = if source.is_dir() {
        Store::import_versions(dir, source, overwrite, now_ms())?
    } else {
        if overwrite {
            return Err(MmError::InvalidArgument(
                "--overwrite は versions/ のフォルダーからの取り込みだけ".into(),
            ));
        }
        Store::create(dir, &load_json_absolute(source)?, now_ms())?
    };
    println!("{}: {} から取り込んだ", dir.display(), source.display());
    finish(store)
}

/// `mm export`: `versions/` に決定的な JSON を書く。`name` があれば名前付きバージョンにする（表に書くので書き手で開く）。
pub fn export(dir: &Path, name: Option<&str>) -> Result<()> {
    let report = match name {
        Some(n) => {
            let store = open_writer(dir)?;
            let report = store.export_named_version(n, ACTOR_KIND, now_ms(), None)?;
            finish(store)?;
            report
        }
        None => open_reader(dir)?.export_versions()?,
    };
    println!(
        "{}: {} ファイル（{} 件を書いた）、ハッシュ {}",
        dir.join(mm_store::VERSIONS_DIR).display(),
        report.files.len(),
        report.written,
        report.hash
    );
    Ok(())
}

/// `mm op`: 操作 1 つか、トランザクション（`--txn`）を適用する。
pub fn op(
    dir: &Path,
    name: Option<&str>,
    tokens: &[String],
    txn_file: Option<&Path>,
) -> Result<()> {
    let txn = match (name, txn_file) {
        (Some(_), Some(_)) => {
            return Err(MmError::InvalidArgument(
                "操作の名前と --txn は同時に渡せない".into(),
            ))
        }
        (None, None) => {
            return Err(MmError::InvalidArgument(format!(
                "操作の名前か --txn を渡す。{}",
                usage()
            )))
        }
        (Some(n), None) => {
            let parsed = parse_op(n, tokens)?;
            let label = parsed.label.unwrap_or_else(|| n.to_owned());
            let mut t = Transaction::new(label, vec![parsed.op]);
            t.base_version = parsed.base;
            t
        }
        (None, Some(file)) => read_txn(file, tokens)?,
    };
    let mut store = open_writer(dir)?;
    let stored = store
        .apply_txn(txn, &cli_context(), actor(), now_ms(), None)
        .map_err(op_error)?;
    println!("バージョン {}: {}", stored.doc_version, stored.name);
    for line in describe_txns(std::slice::from_ref(&stored)) {
        println!("  {line}");
    }
    finish(store)
}

/// `--txn` のファイル（`Transaction` の JSON）を読む。`--base` で元にしたバージョンを上書きできる。
fn read_txn(file: &Path, tokens: &[String]) -> Result<Transaction> {
    let text = std::fs::read_to_string(file)
        .map_err(|e| MmError::NotFound(format!("{}: {e}", file.display())))?;
    let mut txn: Transaction = serde_json::from_str(&text)?;
    if let [flag, value] = tokens {
        if flag != "--base" {
            return Err(MmError::InvalidArgument(format!(
                "--txn と使えない: {flag}"
            )));
        }
        txn.base_version =
            Some(value.parse().map_err(|_| {
                MmError::InvalidArgument(format!("バージョンの番号でない: {value}"))
            })?);
    } else if !tokens.is_empty() {
        return Err(MmError::InvalidArgument(
            "--txn に使えるのは --base <バージョン> だけ".into(),
        ));
    }
    Ok(txn)
}

/// `mm op --schema`: 全部の操作の JSON Schema。
pub fn schema() {
    print!("{}", mm_ops::ops_schema_json());
}

/// `mm undo`。
pub fn undo(dir: &Path) -> Result<()> {
    let mut store = open_writer(dir)?;
    match store
        .undo_last(&cli_context(), actor(), now_ms())
        .map_err(op_error)?
    {
        Some(t) => println!("バージョン {}: {}", t.doc_version, t.name),
        None => println!("取り消すものが無い"),
    }
    finish(store)
}

/// `mm redo`。
pub fn redo(dir: &Path) -> Result<()> {
    let mut store = open_writer(dir)?;
    match store
        .redo_last(&cli_context(), actor(), now_ms())
        .map_err(op_error)?
    {
        Some(t) => println!("バージョン {}: {}", t.doc_version, t.name),
        None => println!("やり直すものが無い"),
    }
    finish(store)
}

/// `mm log`: トランザクションの記録。取り消しとやり直しも、適用したバージョンと種類を添えて 1 行ずつ出す。
pub fn log(dir: &Path) -> Result<()> {
    let store = open_reader(dir)?;
    for txn in store.txn_log()? {
        let kind = match txn.kind {
            TxnKind::Normal => "適用",
            TxnKind::Undo { .. } => "取消",
            TxnKind::Redo { .. } => "再適用",
        };
        let names: Vec<&str> = txn.ops.iter().map(|o| o.name.as_str()).collect();
        println!(
            "バージョン {:>4}  {kind}  {}  {}（{}）  [{}]",
            txn.doc_version,
            format_timestamp(txn.at_ms),
            txn.name,
            txn.actor.kind,
            names.join(", ")
        );
    }
    Ok(())
}

/// `mm diff`: 2 つのバージョンの間の意味の差分。バージョンは番号、`head`、名前付きバージョンの名前。
pub fn diff(dir: &Path, from: &str, to: &str) -> Result<()> {
    let store = open_reader(dir)?;
    let (a, b) = (resolve_version(&store, from)?, resolve_version(&store, to)?);
    if a > b {
        return Err(MmError::InvalidArgument(format!(
            "from（バージョン {a}）が to（バージョン {b}）より先"
        )));
    }
    let lines = describe_txns(&store.txns_between(a, b)?);
    println!("バージョン {a} からバージョン {b}: {} 件", lines.len());
    for line in lines {
        println!("- {line}");
    }
    Ok(())
}

fn resolve_version(store: &Store, spec: &str) -> Result<i64> {
    if let Ok(n) = spec.parse::<i64>() {
        return Ok(n);
    }
    if spec.eq_ignore_ascii_case("head") {
        return Ok(store.doc_version());
    }
    store
        .version_by_name(spec)?
        .map(|v| v.doc_version)
        .ok_or_else(|| {
            MmError::NotFound(format!(
                "バージョン {spec}（番号か head か名前付きバージョン）"
            ))
        })
}
