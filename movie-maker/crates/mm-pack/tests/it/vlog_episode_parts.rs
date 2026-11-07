//! `packs/vlog` の回の頭と締めの `web.cef` の部品（見出しの ToDo、題字、名乗り、次回予告、締めのお願い、縦の動画の終わりの札）と、
//! 画面の注記と線（`marks`）。展開したレイヤーがパックのページをコピーし、引数とテーマを `data` で渡すかを見る。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mm_doc::{Layer, Project};
use mm_ops::{Editor, OpContext};
use mm_pack::{EstimateShaper, Pack};
use mm_types::{ParamValue, RationalTime};
use serde_json::Value;

fn vlog_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/vlog")
}

fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// テンプレートを適用したプロジェクトにマクロ `name` を適用し、置いたレイヤーを返す。
fn place(name: &str, pairs: &[(&str, &str)]) -> (Project, Layer) {
    let pack = Pack::load(&vlog_dir()).expect("packs/vlog を読める");
    let ctx = OpContext::human();
    let mut editor = Editor::new(Project::new());
    let episode = mm_pack::expand(&pack, "episode", &args(&[("n", "4")]), &EstimateShaper).unwrap();
    editor
        .apply(episode, &ctx)
        .expect("テンプレートを適用できる");
    let txn = mm_pack::expand(&pack, name, &args(pairs), &EstimateShaper).expect("展開できる");
    editor.apply(txn, &ctx).expect("マクロを適用できる");
    let project = editor.project().clone();
    let layer = project
        .compositions
        .iter()
        .flat_map(|c| &c.layers)
        .next()
        .expect("レイヤーを 1 つ置く")
        .clone();
    (project, layer)
}

fn html(layer: &Layer) -> &str {
    let ParamValue::Text(html) = &layer.source.params["html"].value else {
        panic!("html は文字");
    };
    html
}

fn data(layer: &Layer) -> &Value {
    let ParamValue::Json(data) = &layer.source.params["data"].value else {
        panic!("data は JSON");
    };
    data
}

fn is_static(layer: &Layer) -> bool {
    layer.source.params["static"].value == ParamValue::Bool(true)
}

/// マクロの引数（名前と値）。
type Pairs = &'static [(&'static str, &'static str)];

/// 部品ごとの最小の引数と、`data` に渡るはずの値。
const PARTS: [(&str, Pairs, (&str, &str)); 7] = [
    (
        "card",
        &[("id", "p"), ("list", "作曲|ミックス"), ("at", "0")],
        ("list", "作曲|ミックス"),
    ),
    (
        "title",
        &[
            ("id", "p"),
            ("kicker", "#4"),
            ("sub", "初音ミク × J-core"),
            ("at", "0"),
        ],
        ("text", "作曲Vlog"),
    ),
    ("name", &[("id", "p"), ("at", "0")], ("sub", "社会人DTMer")),
    (
        "next",
        &[("id", "p"), ("text", "シンセの作り込み"), ("at", "0")],
        ("kicker", "次回"),
    ),
    (
        "cta",
        &[
            ("id", "p"),
            ("text", "高評価"),
            ("x", "1460"),
            ("y", "300"),
            ("at", "0"),
        ],
        ("icon", "like"),
    ),
    (
        "short_end",
        &[("id", "p"), ("sub", "初音ミク × J-core"), ("at", "0")],
        ("text", "続きは本編で！"),
    ),
    (
        "marks",
        &[
            ("id", "p"),
            ("text", "原音"),
            ("pts", "1000,490"),
            ("at", "0"),
        ],
        ("kind", "tag"),
    ),
];

#[test]
fn 回の頭と締めの部品は_web_cef_のレイヤーでページと引数とテーマを渡す() {
    for (name, pairs, (key, want)) in PARTS {
        let (_, layer) = place(name, pairs);
        assert_eq!(layer.source.kind, "web.cef", "{name}");
        assert!(
            html(&layer).contains("window.mmSetTime"),
            "{name}: パックの web/{name}.html をコピーする"
        );
        let data = data(&layer);
        assert_eq!(data[key], want, "{name}: {key} の既定か引数");
        assert_eq!(
            data["theme"]["colors"]["yellow"], "#FFD43B",
            "{name}: テーマの色"
        );
        assert_eq!(
            data["theme"]["fonts"]["title"], "Dela Gothic One",
            "{name}: テーマの書体"
        );
        assert_eq!(
            layer.opacity.keyframes.len(),
            4,
            "{name}: 出る動きと消える動き"
        );
    }
}

#[test]
fn 止まらない部品だけがページを時刻で描く() {
    // 題字のきらめきと名乗りの帯の速度線は出た後も動く（vlog の決まり）。ほかは止まり、動きはレイヤーの不透明度で付ける。
    for (name, pairs, _) in PARTS {
        let (_, layer) = place(name, pairs);
        let animated = matches!(name, "title" | "name" | "short_end");
        assert_eq!(is_static(&layer), !animated, "{name}");
    }
}

#[test]
fn 縦の動画の終わりの札は既定でショートに置く() {
    let (project, _) = place(
        "short_end",
        &[("id", "se"), ("sub", "x"), ("at", "1"), ("dur", "2")],
    );
    let short = project
        .compositions
        .iter()
        .find(|c| c.id.as_str() == "short")
        .expect("ショートのコンポジション");
    assert_eq!(short.layers.len(), 1);
    let layer = &short.layers[0];
    assert_eq!(layer.start, RationalTime::seconds(1));
    assert_eq!(layer.out_point, RationalTime::seconds(2));
    assert_eq!(data(layer)["cta_y"], 560, "数の引数は数のまま渡す");
}

#[test]
fn 見出しの_todo_は今の行を数で渡し_既定は右上() {
    let (_, layer) = place(
        "card",
        &[("id", "c"), ("list", "a|b|c"), ("no", "3"), ("at", "0")],
    );
    let data = data(&layer);
    assert_eq!(data["no"], 3);
    assert_eq!(data["side"], "right");
    assert_eq!(data["plain"], "false");
    assert_eq!(layer.out_point, RationalTime::seconds(4), "既定の長さ 4 秒");
}

#[test]
fn 画面の注記は点と矩形を文字のまま渡す() {
    let (_, layer) = place(
        "marks",
        &[
            ("id", "m"),
            ("kind", "frame"),
            ("box", "10,20,30,40"),
            ("dash", "true"),
            ("color", "yellow"),
            ("at", "2"),
        ],
    );
    let data = data(&layer);
    assert_eq!(data["kind"], "frame");
    assert_eq!(data["box"], "10,20,30,40");
    assert_eq!(data["dash"], "true");
    assert_eq!(data["color"], "yellow");
    assert_eq!(data["pts"], "", "既定は空");
}

/// ページの約束（mm-nodes-cef の README）を破る書き方。時計と乱数と CSS の動きは実時間で進み、外の資源は CSP が読ませない。
const FORBIDDEN: [&str; 8] = [
    "Date.now",
    "performance.now",
    "Math.random",
    "@keyframes",
    "animation:",
    "transition:",
    "src=\"http",
    "url(http",
];

#[test]
fn web_の部品のページは時計と乱数と_css_の動きと外の資源を使わない() {
    let dir = vlog_dir().join("web");
    let mut pages = 0;
    for entry in std::fs::read_dir(&dir).expect("web/ を読める") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("html") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for word in FORBIDDEN {
            assert!(
                !text.contains(word),
                "{}: {word} を使っている",
                path.display()
            );
        }
        assert!(text.contains("window.mmSetTime"), "{}", path.display());
        pages += 1;
    }
    assert!(pages >= PARTS.len(), "部品のページが揃っている: {pages}");
}
