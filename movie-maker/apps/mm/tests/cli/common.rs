//! 結合テストの共通部分。`mm` の実行ファイルを呼ぶ。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// `mm` の実行ファイル。
pub fn mm_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mm"))
}

/// `mm` を呼ぶ。成否は呼ぶ側が見る。
pub fn mm(args: &[&str]) -> Output {
    mm_command().args(args).output().expect("mm を実行できる")
}

/// `mm` を呼び、成功を確かめる。
pub fn mm_ok(args: &[&str]) -> Output {
    let out = mm(args);
    assert!(
        out.status.success(),
        "mm {args:?} が失敗した: {}{}",
        stdout(&out),
        stderr(&out)
    );
    out
}

pub fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// 段階 0 のフィクスチャのプロジェクト（1 つの JSON）。
pub fn stage0_json() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/projects/stage0/project.json")
}

/// 秒の有理数。
fn secs(n: i64) -> serde_json::Value {
    serde_json::json!({"num": n, "den": 1})
}

fn cut_json(id: &str, out_secs: i64) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "source": {"kind": "solid", "version": 1, "params": {}, "inputs": []},
        "in": secs(0),
        "out": secs(out_secs),
    })
}

/// `addSequence --json` に渡す、カット c1（4 秒）と c2（2 秒）を持つシーケンス s1。
pub fn sequence_json() -> String {
    serde_json::json!({"sequence": {"id": "s1", "cuts": [cut_json("c1", 4), cut_json("c2", 2)]}})
        .to_string()
}
