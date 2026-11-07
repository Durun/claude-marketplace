//! `transition.wgsl` をシーケンスの境で描く。レンダラーが進み具合と長さを渡すこと、`rewind` で出ていくカットを
//! 逆向きに描くことを、画素の値（線形の光）で確かめる。GPU が無い環境ではスキップする。

use crate::common;

use common::{comp, gpu, layer, project, render_at, renderer, seconds, shader};
use mm_doc::{Cut, Node, Project, Sequence, Transition};
use mm_types::{Id, ParamValue, RationalTime, Size};

const SMALL: Size = Size::new(64, 36);
/// f16 の丸めを見込んだ許容。
const TOLERANCE: f32 = 3e-3;

/// 時刻を色にするカット。赤 = 素材の秒 / 4。
const CLOCK: &str = "fn mm_main(uv: vec2<f32>) -> vec4<f32> {\n    return vec4<f32>(u.time / 4.0, u.p0.x, 0.0, 1.0);\n}\n";

fn transition(code: &str, rewind: f64) -> Transition {
    Transition {
        node: Node::new("transition.wgsl", 1)
            .with_param("code", ParamValue::Text(code.into()))
            .with_param("rewind", ParamValue::Number(rewind)),
        duration: seconds(1),
    }
}

/// 2 秒のカット a（緑 0）と b（緑 1）。b の頭に `tr`。掛かるのは 1.5..2.5 秒。
fn two_cuts(tr: Transition) -> Project {
    let cut = |id: &str, green: f64| {
        let node = shader(CLOCK).with_param("p0", ParamValue::Number(green));
        Cut::new(id, node, seconds(0), seconds(2))
    };
    let mut b = cut("b", 1.0);
    b.transition_in = Some(tr);
    let seq = Sequence {
        id: Id::new("s"),
        cuts: vec![cut("a", 0.0), b],
        origin: None,
    };
    let node = Node::new("sequence", 1).with_param("sequence", ParamValue::Text("s".into()));
    project(
        vec![comp("main", SMALL, 4, vec![layer("seq", node, 4)])],
        vec![seq],
    )
}

fn at(num: i128, den: u32) -> RationalTime {
    RationalTime::new(num, den).unwrap()
}

fn near(got: f32, want: f32, what: &str) {
    assert!(
        (got - want).abs() <= TOLERANCE,
        "{what}: {want} のはずが {got}"
    );
}

#[test]
fn 進み具合と長さと縦横比を受ける() {
    let Some(ctx) = gpu() else { return };
    let code = "fn mm_transition(uv: vec2<f32>) -> vec4<f32> {\n    return vec4<f32>(u.progress, u.duration / 4.0, u.ratio / 4.0, u.time);\n}\n";
    let mut r = renderer(&ctx);
    let img = render_at(
        &mut r,
        &two_cuts(transition(code, 0.0)),
        "main",
        at(7, 4),
        SMALL,
    );
    let px = img.pixel(10, 10);
    near(px.r, 0.25, "progress");
    near(px.g, 0.25, "duration");
    near(px.b, 64.0 / 36.0 / 4.0, "ratio");
    near(px.a, 0.25, "time");
    assert!(r.take_errors().is_empty());
}

#[test]
fn rewind_が正なら出ていくカットを掛かり始めから逆向きに描く() {
    let Some(ctx) = gpu() else { return };
    let from = "fn mm_transition(uv: vec2<f32>) -> vec4<f32> {\n    return mm_from(uv);\n}\n";
    let mut r = renderer(&ctx);
    // 1.75 秒は掛かり始め（1.5 秒）の 0.25 秒後。普通なら素材の 1.75 秒、3 倍の逆向きなら 1.5 - 0.75 = 0.75 秒。
    let plain = render_at(
        &mut r,
        &two_cuts(transition(from, 0.0)),
        "main",
        at(7, 4),
        SMALL,
    );
    near(plain.pixel(10, 10).r, 1.75 / 4.0, "普通の向き");
    let back = render_at(
        &mut r,
        &two_cuts(transition(from, 3.0)),
        "main",
        at(7, 4),
        SMALL,
    );
    near(back.pixel(10, 10).r, 0.75 / 4.0, "逆向き");
    // カットの頭より前には戻らない。
    let floor = render_at(
        &mut r,
        &two_cuts(transition(from, 9.0)),
        "main",
        at(9, 4),
        SMALL,
    );
    near(floor.pixel(10, 10).r, 0.0, "カットの頭で止まる");
    assert!(r.take_errors().is_empty());
}

#[test]
fn コンパイルに失敗したらエラーを記録する() {
    let Some(ctx) = gpu() else { return };
    let mut r = renderer(&ctx);
    let broken = "fn mm_transition(uv: vec2<f32>) -> vec4<f32> { return nope; }";
    render_at(
        &mut r,
        &two_cuts(transition(broken, 0.0)),
        "main",
        at(2, 1),
        SMALL,
    );
    let errors = r.take_errors();
    assert!(
        errors.iter().any(|e| e.contains("WGSL")),
        "コンパイルの失敗が残る: {errors:?}"
    );
}
