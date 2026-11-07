//! `packs/vlog` を読み、検査し、展開して当てる。壊したパックは検査が捕まえる。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mm_doc::Project;
use mm_ops::{Editor, OpContext, Transaction};
use mm_pack::{check, EstimateShaper, Pack};
use mm_types::{MmError, ParamValue, RationalTime};

fn vlog_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/vlog")
}

fn vlog() -> Pack {
    Pack::load(&vlog_dir()).expect("packs/vlog を読める")
}

/// 見積もりのシェーパーで展開する（フォントを読まない）。
fn expand_est(
    pack: &Pack,
    name: &str,
    args: &BTreeMap<String, String>,
) -> mm_types::Result<Transaction> {
    mm_pack::expand(pack, name, args, &EstimateShaper)
}

fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// 雛形を当てたプロジェクトに `txn` を当てる。
fn apply_on_episode(pack: &Pack, txn: Transaction) -> Project {
    let ctx = OpContext::human();
    let mut editor = Editor::new(Project::new());
    let episode = expand_est(pack, "episode", &args(&[("n", "4")])).unwrap();
    editor.apply(episode, &ctx).expect("雛形を当てられる");
    editor.apply(txn, &ctx).expect("マクロを当てられる");
    editor.project().clone()
}

#[test]
fn vlog_のパックは検査を通る() {
    let pack = vlog();
    let report = check(&pack);
    assert!(report.ok(), "{:#?}", report.errors);
    assert_eq!(report.applied, pack.macros.len());
    assert!(pack.mixer.is_some(), "ダッキングの設定を持つ");
}

#[test]
fn 字幕は札の上に文字を置き_表記を直す() {
    let pack = vlog();
    let txn = expand_est(
        &pack,
        "caption",
        &args(&[("id", "c1"), ("text", "クロードに頼んだ"), ("at", "2")]),
    )
    .unwrap();
    assert_eq!(txn.name, "字幕（vlog）");
    let project = apply_on_episode(&pack, txn);
    let main = &project.compositions[0];
    let ids: Vec<&str> = main.layers.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(ids, ["c1.box", "c1"], "札が下、文字が上");
    let text = &main.layers[1];
    assert_eq!(text.source.kind, "text");
    let value = &text.source.params["text"].value;
    assert_eq!(value, &ParamValue::Text("Claudeに頼んだ".into()));
    assert_eq!(text.start, RationalTime::seconds(2));
    assert_eq!(text.out_point, RationalTime::seconds(3), "既定の長さ 3 秒");
}

#[test]
fn ラベルは出る動きと消える動きを持ち_札の中心で傾く() {
    let pack = vlog();
    let txn = expand_est(
        &pack,
        "label",
        &args(&[("id", "l1"), ("text", "途中経過"), ("at", "1")]),
    )
    .unwrap();
    let project = apply_on_episode(&pack, txn);
    let label = &project.compositions[0].layers[1];
    let times: Vec<f64> = label
        .opacity
        .keyframes
        .iter()
        .map(|k| k.time.to_f64())
        .collect();
    assert_eq!(times, [1.0, 1.2, 3.8, 4.0]);
    let plate = &project.compositions[0].layers[0];
    assert_eq!(plate.transform.rotation, label.transform.rotation);
    assert_eq!(plate.transform.anchor, label.transform.anchor);
    assert!(label.transform.anchor.is_some());
}

#[test]
fn セクションは札とチャプターのマーカーを置き_カットに接続しない() {
    let pack = vlog();
    let txn = expand_est(
        &pack,
        "section",
        &args(&[
            ("id", "s2"),
            ("text", "空間系"),
            ("chapter", "リバーブとディレイ"),
            ("at", "30"),
            ("dur", "60"),
        ]),
    )
    .unwrap();
    let project = apply_on_episode(&pack, txn);
    let main = &project.compositions[0];
    let names: Vec<&str> = main.markers.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["リバーブとディレイ"]);
    assert!(main.layers.iter().all(|l| l.attach_to.is_none()));
}

#[test]
fn 雛形は本編とショートのコンポジションとレンダーターゲットを作る() {
    let pack = vlog();
    let txn = expand_est(&pack, "episode", &args(&[("n", "7")])).unwrap();
    let mut editor = Editor::new(Project::new());
    editor.apply(txn, &OpContext::human()).unwrap();
    let p = editor.project();
    let sizes: Vec<(u32, u32)> = p.compositions.iter().map(|c| (c.width, c.height)).collect();
    assert_eq!(sizes, [(1920, 1080), (1080, 1920)]);
    assert_eq!(p.render_targets[0].output_path, "render/ep7.mp4");
    assert!(p.render_targets[0].delivery.chapters);
    assert_eq!(p.render_targets[0].loudness_lufs, Some(-14.0));
}

#[test]
fn 知らないマクロと足りない引数を拒む() {
    let pack = vlog();
    assert!(matches!(
        expand_est(&pack, "nope", &BTreeMap::new()),
        Err(MmError::NotFound(_))
    ));
    let missing = expand_est(&pack, "caption", &args(&[("id", "c1"), ("at", "0")]));
    assert!(matches!(missing, Err(MmError::InvalidArgument(_))));
    let bad_len = expand_est(
        &pack,
        "caption",
        &args(&[("id", "c1"), ("text", "x"), ("at", "0"), ("dur", "0")]),
    );
    assert!(bad_len.is_err(), "長さ 0 は拒む");
}

/// `packs/vlog` を一時フォルダーに写し、`edit` で 1 つのファイルを書き換えて検査する。
fn check_broken(file: &str, edit: impl FnOnce(&mut serde_json::Value)) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&vlog_dir(), dir.path());
    let path = dir.path().join(file);
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    edit(&mut value);
    std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    check(&Pack::load(dir.path()).expect("形は読める")).errors
}

fn copy_dir(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn mentions(errors: &[String], needle: &str) -> bool {
    errors.iter().any(|e| e.contains(needle))
}

#[test]
fn 検査は壊した文字の型を捕まえる() {
    let errors = check_broken("styles.json", |v| {
        v["label"]["color"] = "pink".into();
        v["telop"]["fade_in"] = 2.0.into();
        v["caption"].as_object_mut().unwrap().remove("box");
        v["caption"]["tilt"] = 3.into();
    });
    assert!(mentions(&errors, "pink"), "{errors:?}");
    assert!(mentions(&errors, "fade_in"), "{errors:?}");
    assert!(mentions(&errors, "tilt"), "{errors:?}");
}

#[test]
fn 検査は壊したマクロの参照を捕まえる() {
    let errors = check_broken("macros/telop.json", |v| {
        v["steps"][0]["text"]["text"] = "{{words}}".into();
        v["steps"][0]["text"]["style"] = "sticker".into();
        v["example"]["color"] = "x".into();
    });
    assert!(mentions(&errors, "{{words}}"), "{errors:?}");
    assert!(
        mentions(&errors, "引数 text をどの手順も使わない"),
        "{errors:?}"
    );
    assert!(mentions(&errors, "sticker"), "{errors:?}");
    assert!(mentions(&errors, "example の color"), "{errors:?}");
}

#[test]
fn 検査は当てられない例を捕まえる() {
    let errors = check_broken("macros/note.json", |v| {
        v["params"]["comp"]["default"] = "nowhere".into();
    });
    assert!(mentions(&errors, "note: 例を当てられない"), "{errors:?}");
}

#[test]
fn 名前とファイル名の違いと形式の番号を捕まえる() {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&vlog_dir(), dir.path());
    let path = dir.path().join("macros/telop.json");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace("\"name\": \"telop\"", "\"name\": \"pop\""),
    )
    .unwrap();
    assert!(matches!(
        Pack::load(dir.path()),
        Err(MmError::InvalidArgument(_))
    ));

    let errors = check_broken("pack.json", |v| v["format"] = 99.into());
    assert!(mentions(&errors, "format"), "{errors:?}");
}
