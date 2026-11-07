//! `packs/vlog` を読み、検査し、展開して適用する。壊したパックは検査が捕まえる。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mm_doc::Project;
use mm_ops::{Editor, OpContext, Transaction};
use mm_pack::{check, EstimateShaper, Pack};
use mm_types::{MmError, ParamValue, RationalTime};

fn vlog_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/vlog")
}

pub(crate) fn vlog() -> Pack {
    Pack::load(&vlog_dir()).expect("packs/vlog を読める")
}

/// 見積もりのシェーパーで展開する（フォントを読まない）。
pub(crate) fn expand_est(
    pack: &Pack,
    name: &str,
    args: &BTreeMap<String, String>,
) -> mm_types::Result<Transaction> {
    mm_pack::expand(pack, name, args, &EstimateShaper)
}

pub(crate) fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// テンプレートを適用したプロジェクトに `txn` を適用する。
pub(crate) fn apply_on_episode(pack: &Pack, txn: Transaction) -> Project {
    let ctx = OpContext::human();
    let mut editor = Editor::new(Project::new());
    let episode = expand_est(pack, "episode", &args(&[("n", "4")])).unwrap();
    editor
        .apply(episode, &ctx)
        .expect("テンプレートを適用できる");
    editor.apply(txn, &ctx).expect("マクロを適用できる");
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
fn 字幕はプレートの上に文字を置き_表記を置き換える() {
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
    assert_eq!(ids, ["c1.plate", "c1"], "プレートが下、文字が上");
    let text = &main.layers[1];
    assert_eq!(text.source.kind, "text");
    let value = &text.source.params["text"].value;
    assert_eq!(value, &ParamValue::Text("Claudeに頼んだ".into()));
    assert_eq!(text.start, RationalTime::seconds(2));
    assert_eq!(text.out_point, RationalTime::seconds(3), "既定の長さ 3 秒");
}

#[test]
fn ラベルは出る動きと消える動きを持ち_プレートの中心で傾く() {
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
fn セクションはプレートとチャプターのマーカーを置き_カットに接続しない() {
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
fn テンプレートは本編とショートのコンポジションとレンダーターゲットを作る() {
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
fn マクロの適用は_1_回の取り消しで全部戻る() {
    let pack = vlog();
    let ctx = OpContext::human();
    let mut editor = Editor::new(Project::new());
    let episode = expand_est(&pack, "episode", &args(&[("n", "4")])).unwrap();
    editor.apply(episode, &ctx).unwrap();
    let before = editor.project().clone();
    let section = args(&[
        ("id", "s1"),
        ("text", "ミックス"),
        ("chapter", "ミックス"),
        ("at", "0"),
        ("dur", "5"),
    ]);
    editor
        .apply(expand_est(&pack, "section", &section).unwrap(), &ctx)
        .unwrap();
    assert_ne!(editor.project(), &before);
    editor.undo(&ctx).unwrap();
    assert_eq!(
        editor.project(),
        &before,
        "プレートと文字とマーカーが全部消える"
    );
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

/// `packs/vlog` を一時フォルダーにコピーし、`edit` で 1 つのファイルを書き換えて検査する。
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
fn 検査は壊した文字のスタイルを捕まえる() {
    let errors = check_broken("styles.json", |v| {
        v["label"]["color"] = "pink".into();
        v["telop"]["fade_in"] = 2.0.into();
        v["caption"].as_object_mut().unwrap().remove("plate");
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
fn 検査は適用できない例を捕まえる() {
    let errors = check_broken("macros/note.json", |v| {
        v["params"]["comp"]["default"] = "nowhere".into();
    });
    assert!(mentions(&errors, "note: 例を適用できない"), "{errors:?}");
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

#[test]
fn テンプレートは本編とショートにミキサーの設定を入れ_取り消し_1_回で戻る() {
    let pack = vlog();
    let ctx = OpContext::human();
    let mut editor = Editor::new(Project::new());
    let episode = expand_est(&pack, "episode", &args(&[("n", "4")])).unwrap();
    editor.apply(episode, &ctx).unwrap();
    let mixer = pack.mixer.clone().expect("vlog は mixer.json を持つ");
    assert!(!editor.project().compositions.is_empty());
    for comp in &editor.project().compositions {
        assert_eq!(comp.mixer.as_ref(), Some(&mixer), "{}", comp.id);
    }
    let ducking = mixer.ducking.as_ref().unwrap();
    assert_eq!(ducking.reduction_db, -11.0);
    assert_eq!(ducking.threshold_db, -40.0);

    editor.undo(&ctx).unwrap();
    assert_eq!(editor.project(), &Project::new(), "コンポジションごと戻る");
}

#[test]
fn 検査は壊したミキサーの設定を捕まえる() {
    let errors = check_broken("mixer.json", |v| {
        v["ducking"]["reduction_db"] = 3.into();
        v["ducking"]["targets"] = serde_json::json!(["voice"]);
        v["ducking"]["threshold_db"] = 6.into();
    });
    assert!(mentions(&errors, "reduction_db"), "{errors:?}");
    assert!(mentions(&errors, "trigger"), "{errors:?}");
    assert!(mentions(&errors, "threshold_db"), "{errors:?}");
}

#[test]
fn 振幅の形のミキサーの設定は_compmixer_として読めず拒まれる() {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&vlog_dir(), dir.path());
    let path = dir.path().join("mixer.json");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("threshold_db", "threshold")).unwrap();
    let Err(MmError::InvalidArgument(message)) = Pack::load(dir.path()) else {
        panic!("CompMixer として読めないパックは拒む");
    };
    assert!(message.contains("mixer.json"), "{message}");
}

#[test]
fn mixer_json_が無いパックでは_mixer_手順を検査が捕まえる() {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&vlog_dir(), dir.path());
    std::fs::remove_file(dir.path().join("mixer.json")).unwrap();
    let report = check(&Pack::load(dir.path()).unwrap());
    assert!(
        mentions(&report.errors, "mixer.json が無い"),
        "{:?}",
        report.errors
    );
}

#[test]
fn 吹き出しは_web_cef_のレイヤーで_theme_と引数をページに渡す() {
    let pack = vlog();
    let txn = expand_est(
        &pack,
        "callout",
        &args(&[
            ("id", "co1"),
            ("text", "ここ"),
            ("x", "1200"),
            ("y", "640"),
            ("at", "2"),
        ]),
    )
    .unwrap();
    let project = apply_on_episode(&pack, txn);
    let layer = &project.compositions[0].layers[0];
    assert_eq!(
        (layer.id.as_str(), layer.source.kind.as_str()),
        ("co1", "web.cef")
    );
    let params = &layer.source.params;
    let ParamValue::Text(html) = &params["html"].value else {
        panic!("html は文字");
    };
    assert!(
        html.contains("window.mmSetTime"),
        "パックの web/callout.html をコピーする"
    );
    let ParamValue::Json(data) = &params["data"].value else {
        panic!("data は JSON");
    };
    assert_eq!(data["text"], "ここ");
    assert_eq!(data["x"], 1200, "数の引数は数のまま渡す");
    assert_eq!(data["side"], "right", "既定の側");
    assert_eq!(
        data["theme"]["colors"]["ink"], "#0B1230",
        "テーマの色を足す"
    );
    assert_eq!(
        params["static"].value,
        ParamValue::Bool(true),
        "動きは不透明度で付ける"
    );
    assert_eq!(layer.opacity.keyframes.len(), 4, "出る動きと消える動き");
    assert_eq!(layer.out_point, RationalTime::seconds(3), "既定の長さ 3 秒");
}

#[test]
fn 検査は無いページと長すぎる動きの_web_手順を捕まえる() {
    let errors = check_broken("macros/callout.json", |v| {
        v["steps"][0]["web"]["page"] = "nothing".into();
        v["steps"][0]["web"]["fade_in"] = 5.0.into();
    });
    assert!(mentions(&errors, "web/nothing.html が無い"), "{errors:?}");
    assert!(mentions(&errors, "fade_in"), "{errors:?}");
}
