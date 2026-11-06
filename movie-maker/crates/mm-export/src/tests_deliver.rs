//! 納品物（ラウドネスの正規化、字幕、チャプター）の、エンジンを通した振る舞い。中身の確かめは `mm` の通しの試験。

use super::*;
use mm_doc::{Delivery, SubtitleOutput};
use mm_runtime::{assemble, RuntimeConfig};
use mm_testkit::decode::SampleMedia;
use mm_testkit::fixtures::vertical_project;

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("mm_export_deliver_{name}_{}", std::process::id()))
}

fn project_with(out: &Path, edit: impl FnOnce(&mut mm_doc::RenderTarget)) -> Project {
    let (mut project, _, _) = vertical_project(&SampleMedia::default(), &temp("unused.json"));
    project.render_targets[0].output_path = out.to_string_lossy().into_owned();
    edit(&mut project.render_targets[0]);
    project
}

#[test]
fn 正規化を持たない音の実装でラウドネスの目標があると拒否する() {
    let out = temp("loud").join("out.json");
    let project = project_with(&out, |t| t.loudness_lufs = Some(-14.0));
    let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
    let err = export_project(&mut engine, &project, &ExportOptions::default()).unwrap_err();
    assert!(matches!(err, MmError::Unsupported(_)), "{err}");
    assert!(!out.exists(), "失敗したら出力を残さない");
    let _ = std::fs::remove_dir_all(temp("loud"));
}

#[test]
fn 字幕を出す設定で文字起こしが無いと拒否する() {
    let out = temp("nosub").join("out.json");
    let project = project_with(&out, |t| {
        t.delivery = Delivery {
            subtitles: SubtitleOutput::Sidecar,
            ..Delivery::default()
        }
    });
    let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
    let err = export_project(&mut engine, &project, &ExportOptions::default()).unwrap_err();
    assert!(matches!(err, MmError::InvalidArgument(_)), "{err}");
    let _ = std::fs::remove_dir_all(temp("nosub"));
}

#[test]
fn 出さなくなった字幕とチャプターの前回のファイルを消し_警告に出す() {
    let dir = temp("stale");
    let out = dir.join("out.json");
    std::fs::create_dir_all(&dir).unwrap();
    let stale = ["out.srt", "out.vtt", "out.chapters.txt"].map(|n| dir.join(n));
    let other = dir.join("out.ja.srt");
    for p in stale.iter().chain([&other]) {
        std::fs::write(p, "old").unwrap();
    }
    let project = project_with(&out, |_| {});
    let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
    let summary = export_project(&mut engine, &project, &ExportOptions::default()).unwrap();
    for p in &stale {
        assert!(!p.exists(), "{} が残っている", p.display());
    }
    assert!(other.exists(), "自分が作る名前でないファイルは消さない");
    assert_eq!(
        summary
            .delivery
            .warnings
            .iter()
            .filter(|w| w.contains("前回"))
            .count(),
        stale.len(),
        "{:?}",
        summary.delivery.warnings
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn チャプターの決まりを満たさなくても書き出しは成功し_理由を警告に出す() {
    let out = temp("chap").join("out.json");
    let project = project_with(&out, |t| {
        t.delivery = Delivery {
            chapters: true,
            ..Delivery::default()
        }
    });
    let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
    let summary = export_project(&mut engine, &project, &ExportOptions::default()).unwrap();
    assert!(summary.delivery.chapters_file.is_none());
    assert!(
        summary
            .delivery
            .warnings
            .iter()
            .any(|w| w.contains("3 つ以上")),
        "{:?}",
        summary.delivery.warnings
    );
    let _ = std::fs::remove_dir_all(temp("chap"));
}
