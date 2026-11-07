//! ノードの種類（効果、`shader.wgsl`、`sequence`）をレンダラーを通して確かめる。画素の値を線形の光で見る。
//! GPU が無い環境ではスキップし、`MM_REQUIRE_GPU` があれば失敗にする。

use crate::common;

use common::{
    comp, dissolve, effect, gpu, layer, project, render_at, renderer, seconds, shader, solid,
};
use mm_doc::{Cut, Node, Sequence};
use mm_render_api::KEPT_RENDERED_FRAMES;
use mm_testkit::fixtures::{invert_lut_asset, INVERT_LUT_ID};
use mm_types::{Id, ParamValue, Rgba, RgbaImage, Size};

/// f16 の丸め（約 5e-4）と線形補間の誤差を見込んだ許容。
const TOLERANCE: f32 = 3e-3;
const SMALL: Size = Size::new(64, 36);

fn assert_pixel(img: &RgbaImage, x: u32, y: u32, want: Rgba) {
    let got = img.pixel(x, y);
    let diff = [
        got.r - want.r,
        got.g - want.g,
        got.b - want.b,
        got.a - want.a,
    ];
    assert!(
        diff.iter().all(|d| d.abs() <= TOLERANCE),
        "({x}, {y}) が {want:?} のはずが {got:?}"
    );
}

fn adjust(name: &str, value: f64) -> Node {
    Node::new("color.adjust", 1).with_param(name, ParamValue::Number(value))
}

fn sequence_layer(id: &str, sequence: &str, len: i64) -> mm_doc::Layer {
    layer(
        id,
        Node::new("sequence", 1).with_param("sequence", ParamValue::Text(sequence.into())),
        len,
    )
}

/// 赤の 2 秒、青の 2 秒（`transition_in` つき）の並び。
fn red_blue(transition_secs: i64) -> Sequence {
    let cut = |id: &str, color: Rgba| Cut::new(id, solid(color), seconds(0), seconds(2));
    let mut blue = cut("b", Rgba::new(0.0, 0.0, 1.0, 1.0));
    if transition_secs > 0 {
        blue.transition_in = Some(dissolve(seconds(transition_secs)));
    }
    Sequence {
        id: Id::new("s"),
        cuts: vec![cut("a", Rgba::new(1.0, 0.0, 0.0, 1.0)), blue],
        origin: None,
    }
}

fn seq_project(seq: Sequence) -> mm_doc::Project {
    project(
        vec![comp("main", SMALL, 6, vec![sequence_layer("seq", "s", 4)])],
        vec![seq],
    )
}

#[test]
fn 明るさは線形の光で露出を上げる() {
    let Some(ctx) = gpu() else { return };
    let mut l = layer("l", solid(Rgba::new(0.1, 0.2, 0.4, 1.0)), 2);
    l.effects.push(effect("e", adjust("brightness", 1.0)));
    let p = project(vec![comp("main", SMALL, 2, vec![l])], vec![]);
    let mut r = renderer(&ctx);
    let img = render_at(&mut r, &p, "main", seconds(0), SMALL);
    assert_pixel(&img, 10, 10, Rgba::new(0.2, 0.4, 0.8, 1.0));
    assert!(r.take_errors().is_empty());
}

#[test]
fn 半透明の画素は色だけが変わりアルファは保たれる() {
    let Some(ctx) = gpu() else { return };
    let mut l = layer("l", solid(Rgba::from_straight(0.1, 0.2, 0.3, 0.5)), 2);
    l.effects.push(effect("e", adjust("brightness", 1.0)));
    let p = project(vec![comp("main", SMALL, 2, vec![l])], vec![]);
    let mut r = renderer(&ctx);
    let img = render_at(&mut r, &p, "main", seconds(0), SMALL);
    assert_pixel(&img, 3, 3, Rgba::from_straight(0.2, 0.4, 0.6, 0.5));
}

#[test]
fn 無効にした効果は掛からない() {
    let Some(ctx) = gpu() else { return };
    let mut l = layer("l", solid(Rgba::new(0.1, 0.2, 0.4, 1.0)), 2);
    let mut e = effect("e", adjust("brightness", 1.0));
    e.enabled = false;
    l.effects.push(e);
    let p = project(vec![comp("main", SMALL, 2, vec![l])], vec![]);
    let mut r = renderer(&ctx);
    let img = render_at(&mut r, &p, "main", seconds(0), SMALL);
    assert_pixel(&img, 10, 10, Rgba::new(0.1, 0.2, 0.4, 1.0));
}

#[test]
fn 効果は入れ子のコンポジションの描き終えた画像に掛かる() {
    let Some(ctx) = gpu() else { return };
    let inner = comp(
        "inner",
        SMALL,
        2,
        vec![layer("s", solid(Rgba::new(0.1, 0.2, 0.4, 1.0)), 2)],
    );
    let mut nested = layer(
        "n",
        Node::new("composition", 1).with_param("composition", ParamValue::Text("inner".into())),
        2,
    );
    nested.effects.push(effect("e", adjust("brightness", 1.0)));
    let p = project(vec![inner, comp("main", SMALL, 2, vec![nested])], vec![]);
    let mut r = renderer(&ctx);
    let img = render_at(&mut r, &p, "main", seconds(0), SMALL);
    assert_pixel(&img, 20, 20, Rgba::new(0.2, 0.4, 0.8, 1.0));
    assert!(r.take_errors().is_empty());
}

#[test]
fn 効果を重ねると上から順に掛かる() {
    let Some(ctx) = gpu() else { return };
    let mut l = layer("l", solid(Rgba::new(0.1, 0.1, 0.1, 1.0)), 2);
    l.effects.push(effect("e1", adjust("brightness", 1.0)));
    l.effects.push(effect("e2", adjust("brightness", 1.0)));
    let p = project(vec![comp("main", SMALL, 2, vec![l])], vec![]);
    let mut r = renderer(&ctx);
    let img = render_at(&mut r, &p, "main", seconds(0), SMALL);
    assert_pixel(&img, 5, 5, Rgba::new(0.4, 0.4, 0.4, 1.0));
}

#[test]
fn lut_は入力の色空間に符号化して引く() {
    let Some(ctx) = gpu() else { return };
    let lut = Node::new("color.lut", 1)
        .with_param("lut", ParamValue::AssetRef(Id::new(INVERT_LUT_ID)))
        .with_param("input_space", ParamValue::Choice("lin_rec709".into()));
    let mut l = layer("l", solid(Rgba::new(0.2, 0.4, 0.8, 1.0)), 2);
    l.effects.push(effect("e", lut));
    let mut p = project(vec![comp("main", SMALL, 2, vec![l])], vec![]);
    p.assets.push(invert_lut_asset());
    let mut r = renderer(&ctx);
    let img = render_at(&mut r, &p, "main", seconds(0), SMALL);
    assert_pixel(&img, 1, 1, Rgba::new(0.8, 0.6, 0.2, 1.0));
}

#[test]
fn 色空間の変換は色の変換の実装を使う() {
    let Some(ctx) = gpu() else { return };
    let convert = Node::new("color.convert", 1)
        .with_param("from", ParamValue::Text("lin_rec709".into()))
        .with_param("to", ParamValue::Text("srgb".into()));
    let mut l = layer("l", solid(Rgba::new(0.2, 0.2, 0.2, 1.0)), 2);
    l.effects.push(effect("e", convert));
    let p = project(vec![comp("main", SMALL, 2, vec![l])], vec![]);
    let mut r = renderer(&ctx);
    let img = render_at(&mut r, &p, "main", seconds(0), SMALL);
    let encoded = 1.055 * 0.2_f32.powf(1.0 / 2.4) - 0.055;
    assert_pixel(&img, 1, 1, Rgba::new(encoded, encoded, encoded, 1.0));
}

#[test]
fn shader_wgsl_は時刻とパラメータを受けて描く() {
    let Some(ctx) = gpu() else { return };
    let code = "fn mm_main(uv: vec2<f32>) -> vec4<f32> { return vec4<f32>(u.p0.x, u.time * 0.25, 0.0, 1.0); }";
    let node = shader(code).with_param("p0", ParamValue::Number(0.5));
    let p = project(
        vec![comp("main", SMALL, 4, vec![layer("l", node, 4)])],
        vec![],
    );
    let mut r = renderer(&ctx);
    let img = render_at(&mut r, &p, "main", seconds(2), SMALL);
    assert_pixel(&img, 7, 7, Rgba::new(0.5, 0.5, 0.0, 1.0));
    assert!(r.take_errors().is_empty());
}

#[test]
fn shader_wgsl_は入力を引ける() {
    let Some(ctx) = gpu() else { return };
    let code = "fn mm_main(uv: vec2<f32>) -> vec4<f32> { return mm_input(uv) * 0.5; }";
    let mut l = layer("l", solid(Rgba::new(0.4, 0.8, 0.2, 1.0)), 2);
    l.effects.push(effect("e", shader(code)));
    let p = project(vec![comp("main", SMALL, 2, vec![l])], vec![]);
    let mut r = renderer(&ctx);
    let img = render_at(&mut r, &p, "main", seconds(0), SMALL);
    assert_pixel(&img, 9, 9, Rgba::new(0.2, 0.4, 0.1, 0.5));
}

#[test]
fn shader_wgsl_のコンパイルに失敗したらそのレイヤーだけ透明でエラーを返す() {
    let Some(ctx) = gpu() else { return };
    let broken = shader("fn mm_main(uv: vec2<f32>) -> vec4<f32> { return nonsense(uv); }");
    let p = project(
        vec![comp(
            "main",
            SMALL,
            2,
            vec![
                layer("base", solid(Rgba::new(0.0, 0.3, 0.0, 1.0)), 2),
                layer("broken", broken, 2),
            ],
        )],
        vec![],
    );
    let mut r = renderer(&ctx);
    for _ in 0..2 {
        let img = render_at(&mut r, &p, "main", seconds(0), SMALL);
        assert_pixel(&img, 4, 4, Rgba::new(0.0, 0.3, 0.0, 1.0));
        let errors = r.take_errors();
        assert_eq!(errors.len(), 1, "壊れたレイヤーの分だけ: {errors:?}");
        assert!(errors[0].contains("broken"), "{errors:?}");
        assert!(errors[0].contains("WGSL"), "{errors:?}");
    }
}

#[test]
fn シーケンスは時刻にあるカットを描く() {
    let Some(ctx) = gpu() else { return };
    let p = seq_project(red_blue(0));
    let mut r = renderer(&ctx);
    let a = render_at(&mut r, &p, "main", seconds(1), SMALL);
    assert_pixel(&a, 5, 5, Rgba::new(1.0, 0.0, 0.0, 1.0));
    let b = render_at(&mut r, &p, "main", seconds(3), SMALL);
    assert_pixel(&b, 5, 5, Rgba::new(0.0, 0.0, 1.0, 1.0));
    let after = render_at(&mut r, &p, "main", seconds(5), SMALL);
    assert_eq!(after.pixel(5, 5).a, 0.0, "シーケンスの長さを過ぎると空");
    assert!(r.take_errors().is_empty());
}

#[test]
fn 無効にしたカットは詰められる() {
    let Some(ctx) = gpu() else { return };
    let mut seq = red_blue(0);
    seq.cuts[0].enabled = false;
    let p = seq_project(seq);
    let mut r = renderer(&ctx);
    let img = render_at(&mut r, &p, "main", seconds(1), SMALL);
    assert_pixel(&img, 5, 5, Rgba::new(0.0, 0.0, 1.0, 1.0));
}

#[test]
fn 境のトランジションは前後に半分ずつ掛かる() {
    let Some(ctx) = gpu() else { return };
    let p = seq_project(red_blue(1));
    let mut r = renderer(&ctx);
    let before = render_at(&mut r, &p, "main", seconds(1), SMALL);
    assert_pixel(&before, 5, 5, Rgba::new(1.0, 0.0, 0.0, 1.0));
    let boundary = render_at(&mut r, &p, "main", seconds(2), SMALL);
    assert_pixel(&boundary, 5, 5, Rgba::new(0.5, 0.0, 0.5, 1.0));
    let t = mm_types::RationalTime::new(7, 4).expect("1.75 秒");
    let early = render_at(&mut r, &p, "main", t, SMALL);
    assert_pixel(&early, 5, 5, Rgba::new(0.75, 0.0, 0.25, 1.0));
    let after = render_at(&mut r, &p, "main", seconds(3), SMALL);
    assert_pixel(&after, 5, 5, Rgba::new(0.0, 0.0, 1.0, 1.0));
    assert!(r.take_errors().is_empty());
}

#[test]
fn 知らないトランジションの種類はカットのままエラーを記録する() {
    let Some(ctx) = gpu() else { return };
    let mut seq = red_blue(1);
    seq.cuts[1].transition_in = Some(mm_doc::Transition {
        node: Node::new("transition.no-such", 1),
        duration: seconds(1),
    });
    let p = seq_project(seq);
    let mut r = renderer(&ctx);
    let img = render_at(&mut r, &p, "main", seconds(2), SMALL);
    assert_pixel(&img, 5, 5, Rgba::new(0.0, 0.0, 1.0, 1.0));
    assert_eq!(r.take_errors().len(), 1);
}

#[test]
fn 効果とトランジションを描いてもテクスチャが溜まらない() {
    let Some(ctx) = gpu() else { return };
    let mut p = seq_project(red_blue(1));
    p.compositions[0].layers[0]
        .effects
        .push(effect("e", adjust("brightness", 1.0)));
    let mut r = renderer(&ctx);
    // 直近のフレームの出力は持っておく決まりなので、それが埋まってから数える。
    for _ in 0..=KEPT_RENDERED_FRAMES {
        render_at(&mut r, &p, "main", seconds(2), SMALL);
    }
    let baseline = r.texture_count();
    for _ in 0..8 {
        render_at(&mut r, &p, "main", seconds(2), SMALL);
    }
    assert_eq!(r.texture_count(), baseline, "毎フレーム解放される");
}
