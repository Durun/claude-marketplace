//! `mm otio export` と `mm otio import`。取り込みが操作として適用され、`mm undo` で取り消せる。

use std::path::{Path, PathBuf};

use super::common::{mm, mm_ok, sequence_json, stderr, stdout};
use tempfile::TempDir;

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// シーケンス s1（単色のカット 2 つ）を足したプロジェクト。
fn project_with_sequence(tmp: &TempDir, name: &str) -> PathBuf {
    let dir = tmp.path().join(name);
    mm_ok(&["new", s(&dir)]);
    mm_ok(&["op", s(&dir), "addSequence", "--json", &sequence_json()]);
    dir
}

fn simple_cut() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/otio/official/simple_cut.otio")
}

#[test]
fn 書き出した_otio_を別のプロジェクトに取り込み_もう一度書き出すと同じになる() {
    let tmp = TempDir::new().unwrap();
    let a = project_with_sequence(&tmp, "a");
    let first = tmp.path().join("first.otio");
    mm_ok(&["otio", "export", s(&a), "--sequence", "s1", "-o", s(&first)]);

    let b = tmp.path().join("b");
    mm_ok(&["new", s(&b)]);
    let out = mm_ok(&["otio", "import", s(&b), s(&first)]);
    assert!(stdout(&out).contains("sequence s1"), "{}", stdout(&out));
    let second = tmp.path().join("second.otio");
    mm_ok(&["otio", "export", s(&b), "-o", s(&second)]);
    assert_eq!(
        std::fs::read_to_string(&first).unwrap(),
        std::fs::read_to_string(&second).unwrap()
    );
}

#[test]
fn 取り込みは操作として記録され_undo_で取り消せる() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("p");
    mm_ok(&["new", s(&dir)]);
    let out = mm_ok(&["otio", "import", s(&dir), s(&simple_cut())]);
    assert!(stdout(&out).contains("OTIO の取り込み"), "{}", stdout(&out));
    assert!(stdout(&mm_ok(&["log", s(&dir)])).contains("OTIO の取り込み"));
    let exported = mm(&[
        "otio",
        "export",
        s(&dir),
        "-o",
        s(&tmp.path().join("x.otio")),
    ]);
    assert!(exported.status.success(), "{}", stderr(&exported));

    mm_ok(&["undo", s(&dir)]);
    let after = mm(&["otio", "export", s(&dir)]);
    assert!(!after.status.success(), "取り消した後は sequence が無い");
}

#[test]
fn 同じ_sequence_の取り込みは拒否し_as_で別名にできる() {
    let tmp = TempDir::new().unwrap();
    let a = project_with_sequence(&tmp, "a");
    let file = tmp.path().join("f.otio");
    mm_ok(&["otio", "export", s(&a), "-o", s(&file)]);

    let refused = mm(&["otio", "import", s(&a), s(&file)]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("既に在る"),
        "{}",
        stderr(&refused)
    );
    mm_ok(&["otio", "import", s(&a), s(&file), "--as", "copy"]);
    let out = mm_ok(&["otio", "export", s(&a), "--sequence", "copy"]);
    assert!(stdout(&out).contains("copy-c1"));
}
