//! Python の opentimelineio との往復。Rust が書いたファイルを opentimelineio が読み、書き直したファイルを Rust が読み戻す。
//!
//! opentimelineio が入っていない環境ではスキップする。CI は入れて `MM_REQUIRE_OTIO=1` で必ず実行する。

use std::path::{Path, PathBuf};
use std::process::Command;

use super::common::{apply, sample_project};
use mm_doc::otio::{export_sequence, import_timeline, ImportOptions};
use mm_doc::{Project, SequenceId};
use serde_json::Value;

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/otio/roundtrip.py")
}

/// `python -I` で opentimelineio を読み込めるか。
fn python_has_otio() -> bool {
    Command::new("python")
        .args(["-I", "-c", "import opentimelineio"])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn require() -> bool {
    std::env::var("MM_REQUIRE_OTIO").is_ok_and(|v| v == "1")
}

fn run_python(src: &Path, dst: &Path) -> Value {
    let out = Command::new("python")
        .arg("-I")
        .arg(script())
        .arg(src)
        .arg(dst)
        .output()
        .expect("python を実行できる");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("Python の報告は JSON")
}

#[test]
fn rust_の書いたファイルを_opentimelineio_が読み_書き直したものを_rust_が読み戻す() {
    if !python_has_otio() {
        assert!(!require(), "MM_REQUIRE_OTIO=1 だが opentimelineio が無い");
        eprintln!("opentimelineio が無いのでスキップする");
        return;
    }
    let original = sample_project();
    let dir = std::env::temp_dir().join(format!("mm-otio-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (written, rewritten) = (dir.join("main.otio"), dir.join("main-py.otio"));
    let timeline = export_sequence(&original, &SequenceId::new("main")).unwrap();
    std::fs::write(&written, serde_json::to_string_pretty(&timeline).unwrap()).unwrap();

    let report = run_python(&written, &rewritten);
    assert_eq!(report["video_tracks"], 1);
    assert_eq!(report["markers"], 2);
    assert_eq!(report["meta_kept"], true);
    assert_eq!(report["track_meta_kept"], true);
    let items: Vec<&str> = report["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        items,
        [
            "Clip",
            "Gap",
            "Transition",
            "Clip",
            "Clip",
            "Clip",
            "Transition",
            "Clip"
        ]
    );

    let text = std::fs::read_to_string(&rewritten).unwrap();
    let back: Value = serde_json::from_str(&text).unwrap();
    let imported = import_timeline(&back, &Project::new(), &ImportOptions::default()).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut project = apply(&imported);
    // 短いほうは本編と同じ手順で足し、全体が元と同じになる。
    let short = export_sequence(&original, &SequenceId::new("short")).unwrap();
    std::fs::write(&written, serde_json::to_string(&short).unwrap()).unwrap();
    run_python(&written, &rewritten);
    let back: Value = serde_json::from_str(&std::fs::read_to_string(&rewritten).unwrap()).unwrap();
    let more = import_timeline(&back, &project, &ImportOptions::default()).unwrap();
    project.sequences.extend(more.sequences);
    project.normalize();
    project.validate().unwrap();
    assert_eq!(project, original);
    std::fs::remove_dir_all(&dir).ok();
}
