//! `packs/vlog` の `web.cef` の部品（`web/*.html` と、それを置くマクロ）。展開したレイヤーの形と、ページの約束を確かめる。

use mm_doc::Layer;
use mm_types::{ParamValue, RationalTime};
use serde_json::Value;

use crate::vlog::{apply_on_episode, args, expand_est, vlog};

/// マクロを展開してテンプレートの上に適用し、置いた `web.cef` のレイヤーと、ページに渡す `data` を返す。
fn web_layer(name: &str, pairs: &[(&str, &str)]) -> (Layer, Value) {
    let pack = vlog();
    let txn = expand_est(&pack, name, &args(pairs)).expect("展開できる");
    let project = apply_on_episode(&pack, txn);
    let layer = project.compositions[0].layers[0].clone();
    assert_eq!(
        layer.source.kind.as_str(),
        "web.cef",
        "{name} は web.cef で描く"
    );
    let ParamValue::Json(data) = layer.source.params["data"].value.clone() else {
        panic!("{name}: data は JSON");
    };
    (layer, data)
}

fn is_static(layer: &Layer) -> bool {
    layer.source.params["static"].value == ParamValue::Bool(true)
}

/// マクロの名前、引数、パラメータの組ごとに 1 回だけ描くか。
type StaticCase<'a> = (&'a str, &'a [(&'a str, &'a str)], bool);

#[test]
fn 時刻で中身が変わる部品だけが毎フレーム描かれる() {
    let cases: [StaticCase; 7] = [
        (
            "callout",
            &[
                ("id", "a"),
                ("text", "x"),
                ("x", "1"),
                ("y", "1"),
                ("at", "0"),
            ],
            true,
        ),
        ("tip", &[("id", "a"), ("head", "x"), ("at", "0")], true),
        ("sticker", &[("id", "a"), ("text", "x"), ("at", "0")], true),
        ("claude", &[("id", "a"), ("text", "x"), ("at", "0")], false),
        (
            "figure",
            &[("id", "a"), ("head", "x"), ("labels", "a|b"), ("at", "0")],
            false,
        ),
        (
            "lyrics",
            &[("id", "a"), ("words", "x@0+1"), ("at", "0")],
            false,
        ),
        (
            "read",
            &[("id", "a"), ("tab", "x"), ("doc", "y"), ("at", "0")],
            false,
        ),
    ];
    for (name, pairs, want) in cases {
        let (layer, data) = web_layer(name, pairs);
        assert_eq!(is_static(&layer), want, "{name} の static");
        assert_eq!(
            data["theme"]["colors"]["miku"], "#39C5BB",
            "{name} にテーマの色を渡す"
        );
        assert_eq!(
            layer.opacity.keyframes.len(),
            4,
            "{name} は出る動きと消える動きを持つ"
        );
    }
}

#[test]
fn 採点モード風の歌詞は単語と音符の時刻をそのままページに渡す() {
    let words = "帰り道@0.6+0.6 イヤホン@1.2+0.7 / から@1.9+0.5";
    let notes = "69@0.6+0.3 72@1.55+0.35";
    let (layer, data) = web_layer(
        "lyrics",
        &[
            ("id", "ly"),
            ("words", words),
            ("notes", notes),
            ("at", "2"),
        ],
    );
    assert_eq!(data["words"], words);
    assert_eq!(data["notes"], notes);
    assert_eq!(data["bars"], Value::Null, "省いた小節の線は null");
    assert_eq!(
        (data["x"].clone(), data["y"].clone()),
        (72.into(), 618.into())
    );
    assert_eq!(layer.start, RationalTime::seconds(2));
    assert_eq!(layer.out_point, RationalTime::seconds(6), "既定の長さ 6 秒");

    let (_, bare) = web_layer("lyrics", &[("id", "ly"), ("words", words), ("at", "0")]);
    assert_eq!(bare["notes"], Value::Null, "音符が無ければ帯を出さない");
}

#[test]
fn 図解は行と試聴の_cues_を渡し_右に寄せる() {
    let (_, data) = web_layer(
        "figure",
        &[
            ("id", "fg"),
            ("head", "ディレイで形を保ってぼかす"),
            ("labels", "原音|ディレイ"),
            ("kinds", "dry delay"),
            ("row_at", "0.2 1.2"),
            ("cues", "2@3+1.5"),
            ("at", "1"),
        ],
    );
    assert_eq!(data["labels"], "原音|ディレイ");
    assert_eq!(data["row_at"], "0.2 1.2");
    assert_eq!(data["cues"], "2@3+1.5");
    assert_eq!(data["comments"], Value::Null);
    assert_eq!(
        (data["side"].clone(), data["x"].clone()),
        ("right".into(), 1856.into())
    );
}

#[test]
fn 読む窓と_claude_の吹き出しは既定の場所に置く() {
    let (_, read) = web_layer(
        "read",
        &[
            ("id", "rd"),
            ("tab", "題"),
            ("doc", "> 要点"),
            ("segs", "要点@0+1"),
            ("at", "0"),
        ],
    );
    assert_eq!(
        (read["w"].clone(), read["h"].clone()),
        (1000.into(), 840.into())
    );
    assert_eq!(read["segs"], "要点@0+1");
    let (_, claude) = web_layer("claude", &[("id", "cl"), ("text", "はい"), ("at", "0")]);
    assert_eq!(claude["layout"], "stack");
    assert_eq!(
        (claude["side"].clone(), claude["x"].clone()),
        ("right".into(), 1830.into())
    );
}

#[test]
fn 小ネタとステッカーと吹き出しのしっぽの既定() {
    let (_, tip) = web_layer(
        "tip",
        &[("id", "tp"), ("head", "Pro-C 2｜コンプ"), ("at", "0")],
    );
    assert_eq!(tip["tag"], "ちなみに");
    assert_eq!(
        (tip["side"].clone(), tip["tail"].clone()),
        ("left".into(), "down".into())
    );
    let (_, untagged) = web_layer(
        "tip",
        &[("id", "tp"), ("head", "x"), ("tag", ""), ("at", "0")],
    );
    assert_eq!(untagged["tag"], "", "空の札は出さない");
    let (_, sticker) = web_layer(
        "sticker",
        &[
            ("id", "st"),
            ("text", "シュイーン"),
            ("look", "oto"),
            ("at", "0"),
        ],
    );
    assert_eq!(
        (sticker["look"].clone(), sticker["tilt"].clone()),
        ("oto".into(), (-4).into())
    );
    let (_, callout) = web_layer(
        "callout",
        &[
            ("id", "co"),
            ("text", "x"),
            ("name", "Splice"),
            ("tail", "side"),
            ("x", "5"),
            ("y", "6"),
            ("at", "0"),
        ],
    );
    assert_eq!(
        (callout["name"].clone(), callout["tail"].clone()),
        ("Splice".into(), "side".into())
    );
    let (_, plain) = web_layer(
        "callout",
        &[
            ("id", "co"),
            ("text", "x"),
            ("x", "5"),
            ("y", "6"),
            ("at", "0"),
        ],
    );
    assert_eq!(
        (plain["name"].clone(), plain["tail"].clone()),
        (Value::Null, "down".into())
    );
}

/// 実時間で進むもの（時計、乱数、タイマー、CSS の動き）と、外の資源の読み込み。
const FORBIDDEN: [&str; 9] = [
    "Date.now",
    "performance.now",
    "new Date",
    "Math.random",
    "setTimeout",
    "requestAnimationFrame",
    "@keyframes",
    "animation:",
    "transition:",
];

#[test]
fn web_の部品はページの約束を守る() {
    let pack = vlog();
    assert!(
        pack.pages.len() >= 7,
        "部品のページが揃う: {:?}",
        pack.pages.keys()
    );
    for (name, html) in &pack.pages {
        assert!(
            html.contains("window.mmSetTime"),
            "{name}: mmSetTime で画面を作る"
        );
        for word in FORBIDDEN {
            assert!(
                !html.contains(word),
                "{name}: {word} を使わない（同じ t で画素が変わる）"
            );
        }
        for (i, _) in html.match_indices("src=\"") {
            assert!(
                html[i + 5..].starts_with("data:"),
                "{name}: 資源は data: で埋め込む"
            );
        }
        let external = html
            .match_indices("http")
            .filter(|(i, _)| !html[*i..].starts_with("http://www.w3.org/2000/svg"));
        assert_eq!(external.count(), 0, "{name}: 外の URL を読まない");
    }
}
