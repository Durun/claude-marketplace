//! sequence → OTIO → sequence の往復と、OTIO 側で編集されたときの読み。

use super::common::{apply, sample_project, t};
use mm_doc::otio::{export_sequence, import_timeline, ImportOptions};
use mm_doc::{CompMixer, Ducking, Layer, Node, Project, SequenceId};
use mm_types::{Id, ParamValue, RationalTime};
use serde_json::{json, Value};

fn export(p: &Project, id: &str) -> Value {
    export_sequence(p, &SequenceId::new(id)).unwrap()
}

/// 文字列を経由する（ファイルに書いて読むのと同じ）。
fn through_text(v: &Value) -> Value {
    serde_json::from_str(&serde_json::to_string_pretty(v).unwrap()).unwrap()
}

fn children(timeline: &Value) -> &Vec<Value> {
    timeline["tracks"]["children"][0]["children"]
        .as_array()
        .unwrap()
}

#[test]
fn 本編とショートはプロジェクトのデータとして往復する() {
    let original = sample_project();
    let main = through_text(&export(&original, "main"));
    let first = import_timeline(&main, &Project::new(), &ImportOptions::default()).unwrap();
    assert!(first.warnings.is_empty(), "{:?}", first.warnings);
    let mut project = apply(&first);

    let short = through_text(&export(&original, "short"));
    let second = import_timeline(&short, &project, &ImportOptions::default()).unwrap();
    assert!(second.assets.is_empty(), "同じ場所の素材は足さない");
    project.sequences.extend(second.sequences);
    project.normalize();
    project.validate().unwrap();

    assert_eq!(project, original);
}

#[test]
fn トラックの項目は_otio_の形で並び配置の長さが合う() {
    let p = sample_project();
    let tl = export(&p, "main");
    let kinds: Vec<&str> = children(&tl)
        .iter()
        .map(|c| c["OTIO_SCHEMA"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        [
            "Clip.1",
            "Gap.1",
            "Transition.1",
            "Clip.1",
            "Clip.1",
            "Clip.1",
            "Transition.1",
            "Clip.1"
        ]
    );
    let c1 = &children(&tl)[0];
    assert_eq!(c1["effects"][0]["OTIO_SCHEMA"], "LinearTimeWarp.1");
    assert_eq!(c1["effects"][0]["time_scalar"], 1.5);
    assert_eq!(
        c1["media_reference"]["target_url"],
        "file:///D:/dev/%E6%97%A5%E6%9C%AC%E8%AA%9E%20%E3%81%AE/cam.mp4"
    );
    let disabled = &children(&tl)[4];
    assert_eq!(disabled["enabled"], false);
    assert_eq!(disabled["source_range"]["duration"]["value"], 0.0);
    let gap = &children(&tl)[1];
    assert_eq!(
        gap["source_range"]["duration"]["value"], 20.0,
        "1/3 秒をコンポジションの 60 fps で"
    );
    assert_eq!(
        tl["tracks"]["children"][0]["markers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn 別の_id_で取り込むと_id_を作り直し元との関係とコンポジションのメタデータを使わない() {
    let p = sample_project();
    let tl = through_text(&export(&p, "main"));
    let opts = ImportOptions {
        sequence_id: Some("copy".into()),
    };
    let imported = import_timeline(&tl, &p, &opts).unwrap();
    assert!(imported.assets.is_empty());
    let seq = &imported.sequences[0];
    assert_eq!(seq.id.as_str(), "copy");
    assert_eq!(seq.cuts[0].id.as_str(), "copy-c1");
    let original = p.sequence(&Id::new("main")).unwrap();
    for (a, b) in seq.cuts.iter().zip(&original.cuts) {
        assert_eq!(
            (a.in_point, a.out_point, a.speed, a.enabled),
            (b.in_point, b.out_point, b.speed, b.enabled)
        );
        assert_eq!(a.source, b.source);
    }
    let comp = &imported.compositions[0];
    assert_eq!(comp.id.as_str(), "copy");
    assert_eq!(comp.markers.len(), 2);
    assert_eq!(
        comp.markers[0].time,
        t(1, 2),
        "コンポジションのメタデータが無いので、トラックの時刻のまま"
    );
    let mut merged = p.clone();
    merged.sequences.extend(imported.sequences);
    merged.compositions.extend(imported.compositions);
    merged.normalize();
    merged.validate().unwrap();
}

#[test]
fn 同じ_id_の取り込みは拒否する() {
    let p = sample_project();
    let tl = export(&p, "main");
    let err = import_timeline(&tl, &p, &ImportOptions::default()).unwrap_err();
    assert!(err.to_string().contains("既に在る"), "{err}");
}

#[test]
fn 別のツールで直した値は_メタデータより_otio_の値に従う() {
    let p = sample_project();
    let mut tl = export(&p, "main");
    // c1 のイン点を 0 に、速さを 2 倍速にする。
    let clip = &mut tl["tracks"]["children"][0]["children"][0];
    clip["source_range"]["start_time"]["value"] = json!(0.0);
    clip["effects"][0]["time_scalar"] = json!(2.0);
    let imported = import_timeline(&tl, &Project::new(), &ImportOptions::default()).unwrap();
    let c1 = &imported.sequences[0].cuts[0];
    assert_eq!(c1.id.as_str(), "c1", "ID などメタデータの値は残る");
    assert_eq!(c1.in_point, RationalTime::ZERO);
    assert_eq!(c1.speed, mm_types::Rational::integer(2));
    // 配置の長さは変わらず、素材が進む量が速さで変わる。
    assert_eq!(c1.duration().unwrap(), t(8008, 9000));
}

#[test]
fn 無効にされたクリップは長さを保つ隙間になる() {
    let p = sample_project();
    let mut tl = export(&p, "main");
    tl["tracks"]["children"][0]["children"][3]["enabled"] = json!(false);
    let imported = import_timeline(&tl, &Project::new(), &ImportOptions::default()).unwrap();
    let cut = &imported.sequences[0].cuts[2];
    assert!(cut.source.is_gap() && cut.enabled);
    assert!(imported.warnings.iter().any(|w| w.contains("無効")));
}

#[test]
fn 同じ時刻のマーカーは後ろへずらして昇順にする() {
    let p = sample_project();
    let mut tl = export(&p, "main");
    let markers = &mut tl["tracks"]["children"][0]["markers"];
    markers[1]["marked_range"] = markers[0]["marked_range"].clone();
    let imported = import_timeline(&tl, &Project::new(), &ImportOptions::default()).unwrap();
    let m = &imported.compositions[0].markers;
    assert_eq!(m[1].time, m[0].time.checked_add(t(1, 1000)).unwrap());
    assert!(imported.warnings.iter().any(|w| w.contains("ずらした")));
    apply(&imported);
}

#[test]
fn timeline_でないものと映像の無いものは拒否する() {
    let not_timeline = json!({"OTIO_SCHEMA": "Clip.1"});
    assert!(import_timeline(&not_timeline, &Project::new(), &ImportOptions::default()).is_err());
    let mut tl = export(&sample_project(), "main");
    tl["tracks"]["children"][0]["kind"] = json!("Audio");
    assert!(import_timeline(&tl, &Project::new(), &ImportOptions::default()).is_err());
}

/// JSON の全部の値を 1 つずつ null と極端な値に置き換えても、パニックせず `Ok` か `Err` を返す。
#[test]
fn 壊れた入力でパニックしない() {
    fn collect(v: &Value, here: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
        out.push(here.clone());
        let keys: Vec<String> = match v {
            Value::Object(m) => m.keys().cloned().collect(),
            Value::Array(a) => (0..a.len()).map(|i| i.to_string()).collect(),
            _ => return,
        };
        for k in keys {
            let child = match v {
                Value::Array(a) => &a[k.parse::<usize>().unwrap()],
                _ => &v[k.as_str()],
            };
            here.push(k);
            collect(child, here, out);
            here.pop();
        }
    }
    fn slot<'a>(v: &'a mut Value, path: &[String]) -> Option<&'a mut Value> {
        let mut cur = v;
        for k in path {
            cur = match cur {
                Value::Object(m) => m.get_mut(k)?,
                Value::Array(a) => a.get_mut(k.parse::<usize>().ok()?)?,
                _ => return None,
            };
        }
        Some(cur)
    }
    let base = export(&sample_project(), "main");
    let mut paths = Vec::new();
    collect(&base, &mut Vec::new(), &mut paths);
    for path in paths {
        for replacement in [
            Value::Null,
            json!(1e300),
            json!(1e38),
            json!(-1.0),
            json!("x"),
        ] {
            let mut tl = base.clone();
            if let Some(target) = slot(&mut tl, &path) {
                *target = replacement;
            }
            let _ = import_timeline(&tl, &Project::new(), &ImportOptions::default());
        }
    }
}

/// sequence を置いたコンポジションに、ミキサーと、OTIO に出ない sequence を置いたレイヤーを足す。
fn project_with_mixer() -> Project {
    let mut p = sample_project();
    let comp = &mut p.compositions[0];
    comp.mixer = Some(CompMixer {
        master_db: -2.0,
        ducking: Some(Ducking::voice_over_bgm()),
        ..CompMixer::default()
    });
    let short = Node::new("sequence", 1).with_param("sequence", ParamValue::Text("short".into()));
    comp.layers
        .push(Layer::new("l-short", short, t(0, 1), t(0, 1), t(2, 1)));
    p.validate().unwrap();
    p
}

#[test]
fn ミキサーは往復し_出さない_sequence_を置いたレイヤーは持ち込まない() {
    let p = project_with_mixer();
    let tl = through_text(&export(&p, "main"));
    let imported = import_timeline(&tl, &Project::new(), &ImportOptions::default()).unwrap();
    let back = apply(&imported);
    let comp = &back.compositions[0];
    assert_eq!(comp.mixer, p.compositions[0].mixer);
    let ids: Vec<&str> = comp.layers.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(ids, ["l-main"]);
}

#[test]
fn 取り込む先に同じ_id_のレイヤーがあれば_as_を案内して拒否する() {
    let p = sample_project();
    let mut target = Project::new();
    let mut other = p.compositions[0].clone();
    other.id = Id::new("other");
    target.compositions.push(other);
    let tl = export(&p, "main");
    let err = import_timeline(&tl, &target, &ImportOptions::default()).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("l-main") && msg.contains("--as"), "{msg}");
}

#[test]
fn 別の_id_で取り込むと捨てるコンポジションの設定を警告する() {
    let p = project_with_mixer();
    let tl = export(&p, "main");
    let opts = ImportOptions {
        sequence_id: Some("copy".into()),
    };
    let imported = import_timeline(&tl, &p, &opts).unwrap();
    assert!(
        imported.warnings.iter().any(|w| w.contains("main-comp")),
        "{:?}",
        imported.warnings
    );
}

#[test]
fn windows_の区切りのパスの素材も同じ素材として見つける() {
    let mut p = sample_project();
    p.assets[0].path = "D:\\dev\\日本語 の\\cam.mp4".into();
    let tl = export(&p, "main");
    let opts = ImportOptions {
        sequence_id: Some("copy".into()),
    };
    let imported = import_timeline(&tl, &p, &opts).unwrap();
    assert!(imported.assets.is_empty(), "{:?}", imported.assets);
}

#[test]
fn 読めないメタデータと形のバージョンの違いは警告する() {
    let p = sample_project();
    let mut tl = export(&p, "main");
    tl["tracks"]["children"][0]["metadata"]["movie-maker"]["version"] = json!(99);
    tl["tracks"]["children"][0]["children"][0]["metadata"]["movie-maker"] = json!({"id": 3});
    let imported = import_timeline(&tl, &Project::new(), &ImportOptions::default()).unwrap();
    let warned = |word: &str| imported.warnings.iter().any(|w| w.contains(word));
    assert!(warned("99"), "{:?}", imported.warnings);
    assert!(warned("メタデータが読めない"), "{:?}", imported.warnings);
}
