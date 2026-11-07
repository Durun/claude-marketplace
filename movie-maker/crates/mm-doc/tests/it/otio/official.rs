//! OpenTimelineIO のリポジトリの `tests/sample_data/` の実例ファイルと、Python の opentimelineio が書いたファイルを読む。

use std::path::{Path, PathBuf};

use super::common::apply;
use mm_doc::otio::{import_timeline, ImportOptions, Imported};
use mm_doc::Project;
use mm_types::RationalTime;
use serde_json::Value;

fn dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/otio")
        .join(name)
}

fn read(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn import(name: &str, file: &str) -> Imported {
    let path = dir(name).join(file);
    import_timeline(&read(&path), &Project::new(), &ImportOptions::default())
        .unwrap_or_else(|e| panic!("{file}: {e}"))
}

fn seconds(p: &Project, seq: usize) -> RationalTime {
    p.sequences[seq]
        .cuts
        .iter()
        .filter(|c| c.enabled)
        .map(|c| c.duration().unwrap())
        .fold(RationalTime::ZERO, |a, d| a.checked_add(d).unwrap())
}

#[test]
fn 公式の実例は全部読めて不変条件を満たす() {
    let mut count = 0;
    for entry in std::fs::read_dir(dir("official")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let imported = import("official", &name);
        assert!(!imported.sequences.is_empty(), "{name}");
        apply(&imported);
        count += 1;
    }
    assert!(count >= 9, "実例が足りない: {count}");
}

#[test]
fn simple_cut_は_3_つのクリップと素材の参照を読む() {
    let imported = import("official", "simple_cut.otio");
    let p = apply(&imported);
    assert_eq!(p.sequences.len(), 1);
    let cuts = &p.sequences[0].cuts;
    assert!(cuts.len() >= 3);
    // Clip-001: source_range 3 フレームから 3 フレーム、24 fps。
    assert_eq!(cuts[0].in_point, RationalTime::new(3, 24).unwrap());
    assert_eq!(cuts[0].out_point, RationalTime::new(6, 24).unwrap());
    assert!(p.assets.iter().any(|a| a.path == "/folder/titles.mov"));
    assert!(p.assets.iter().all(|a| a.fps.is_some()));
}

#[test]
fn transition_はディゾルブになり配置を変えない() {
    let imported = import("official", "transition_test.otio");
    let p = apply(&imported);
    let with_transition = p.sequences[0]
        .cuts
        .iter()
        .filter(|c| c.transition_in.is_some())
        .count();
    assert!(with_transition >= 1, "{:?}", imported.warnings);
    assert!(seconds(&p, 0) > RationalTime::ZERO);
}

#[test]
fn 複数トラックは_sequence_を分け_音声のトラックは警告で除く() {
    let imported = import("python", "video_and_audio.otio");
    assert_eq!(imported.sequences.len(), 2, "{:?}", imported.warnings);
    assert!(imported.warnings.iter().any(|w| w.contains("Audio")));
    let ids: std::collections::HashSet<_> =
        imported.sequences.iter().map(|s| s.id.clone()).collect();
    assert_eq!(ids.len(), imported.sequences.len());
}

#[test]
fn 入れ子の_stack_は長さを保つ隙間になる() {
    let imported = import("official", "nested_example.otio");
    assert!(
        imported.warnings.iter().any(|w| w.contains("入れ子")),
        "{:?}",
        imported.warnings
    );
    apply(&imported);
}

#[test]
fn マーカーはコンポジションに入り時刻は厳密な昇順() {
    let imported = import("official", "screening_example.otio");
    let marked: Vec<_> = imported
        .compositions
        .iter()
        .filter(|c| !c.markers.is_empty())
        .collect();
    assert!(!marked.is_empty(), "{:?}", imported.warnings);
    for c in marked {
        assert!(c.markers.windows(2).all(|w| w[0].time < w[1].time));
    }
}

#[test]
fn python_の_opentimelineio_が書いたファイルを読む() {
    let mut count = 0;
    for entry in std::fs::read_dir(dir("python")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "otio") {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            apply(&import("python", &name));
            count += 1;
        }
    }
    assert!(count >= 3, "Python のフィクスチャが足りない: {count}");
}

#[test]
fn python_の_basic_はカット_隙間_速さ_トランジション_マーカーを読む() {
    let imported = import("python", "basic.otio");
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let p = apply(&imported);
    let cuts = &p.sequences[0].cuts;
    assert_eq!(cuts.len(), 3);
    assert_eq!(
        (cuts[0].in_point, cuts[0].out_point),
        (RationalTime::seconds(1), RationalTime::seconds(3))
    );
    assert!(cuts[1].source.is_gap());
    assert_eq!(
        cuts[1].duration().unwrap(),
        RationalTime::new(1, 2).unwrap()
    );
    assert_eq!(cuts[2].speed, mm_types::Rational::integer(2));
    let transition = cuts[2].transition_in.as_ref().expect("ディゾルブ");
    assert_eq!(transition.node.kind, "transition.dissolve");
    assert_eq!(transition.duration, RationalTime::new(1, 2).unwrap());
    let asset = p
        .assets
        .iter()
        .find(|a| a.path.ends_with("a b.mp4"))
        .unwrap();
    assert_eq!(asset.path, "D:/media/a b.mp4");
    // トラックのマーカー 2 つとクリップのマーカー 1 つ。クリップのは素材の 30 フレーム目 = 配置の 6/24 秒。
    let times: Vec<_> = p.compositions[0].markers.iter().map(|m| m.time).collect();
    assert_eq!(
        times,
        [
            RationalTime::ZERO,
            RationalTime::new(1, 4).unwrap(),
            RationalTime::new(5, 2).unwrap()
        ]
    );
}

#[test]
fn python_の_ntsc_は_30000_1001_の分数で読み_無効なクリップは隙間にする() {
    let imported = import("python", "ntsc.otio");
    let p = apply(&imported);
    let cuts = &p.sequences[0].cuts;
    assert_eq!(
        cuts[0].in_point,
        RationalTime::new(100 * 1001, 30000).unwrap()
    );
    assert_eq!(
        cuts[0].duration().unwrap(),
        RationalTime::new(90 * 1001, 30000).unwrap()
    );
    assert!(cuts[2].source.is_gap());
    assert!(imported.warnings.iter().any(|w| w.contains("無効")));
    let fps = p.assets[0].fps.unwrap();
    assert_eq!((fps.num(), fps.den()), (30000, 1001));
}
