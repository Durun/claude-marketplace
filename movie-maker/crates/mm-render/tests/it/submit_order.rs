//! 中間のテクスチャのプールと提出の順序。まだ提出していない合成が読むテクスチャを、次の `fill` や効果が
//! 先に上書きしないことを、画素で確かめる（`mm_gpu::WgpuGpu` のプールの決まり）。
//! GPU が無い環境ではスキップし、`MM_REQUIRE_GPU` があれば失敗にする。

use crate::common;

use common::{
    comp, dissolve, effect, gpu, layer, project, render_at, renderer, seconds, shader, solid,
};
use mm_doc::{Cut, Node, Param, Sequence};
use mm_render::FrameCacheConfig;
use mm_types::{Id, ParamValue, Rgba, RgbaImage, Size};

const SIZE: Size = Size::new(64, 36);
/// f16 の丸めの許容。
const TOLERANCE: f32 = 2e-3;

const RED: Rgba = Rgba::new(0.8, 0.1, 0.1, 1.0);
const BLUE: Rgba = Rgba::new(0.1, 0.2, 0.7, 1.0);
const GREEN: Rgba = Rgba::new(0.1, 0.6, 0.2, 1.0);

fn pixel(img: &RgbaImage, x: u32, y: u32) -> [f32; 4] {
    let i = ((y * img.size().width + x) * 4) as usize;
    std::array::from_fn(|c| img.data()[i + c])
}

fn assert_near(got: [f32; 4], want: [f32; 4], what: &str) {
    let diff = got
        .iter()
        .zip(want)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    assert!(diff <= TOLERANCE, "{what}: {got:?} と {want:?}");
}

fn no_cache() -> FrameCacheConfig {
    FrameCacheConfig {
        memory_bytes: 0,
        store: None,
    }
}

/// 親と同じ大きさの入れ子に「単色 + color.adjust」、その上に親の単色（半分の大きさで中央）。
fn nested_with_effect() -> mm_doc::Project {
    let mut inner_solid = layer("inner-solid", solid(BLUE), 4);
    inner_solid.effects.push(effect(
        "adjust",
        Node::new("color.adjust", 1).with_param("brightness", ParamValue::Number(0.1)),
    ));
    let inner = comp("inner", SIZE, 4, vec![inner_solid]);
    let nested = layer(
        "nested",
        Node::new("composition", 1).with_param("composition", ParamValue::Text("inner".into())),
        4,
    );
    let mut top = layer("top", solid(GREEN), 4);
    top.transform.scale = Param::constant(ParamValue::Number(0.5));
    project(
        vec![inner, comp("main", SIZE, 4, vec![nested, top])],
        vec![],
    )
}

#[test]
fn 入れ子の効果の出力を親の単色が上書きしない() {
    let Some(ctx) = gpu() else { return };
    let p = nested_with_effect();
    // 入れ子だけを描いた画像が正しい色（親の単色が混ざる余地が無い）。
    let want = pixel(
        &render_at(&mut renderer(&ctx), &p, "inner", seconds(1), SIZE),
        0,
        0,
    );
    for (name, mut r) in [
        ("キャッシュあり", renderer(&ctx)),
        (
            "キャッシュなし",
            renderer(&ctx).with_frame_cache(no_cache()),
        ),
    ] {
        for frame in 0..2 {
            let img = render_at(&mut r, &p, "main", seconds(1), SIZE);
            assert_near(
                pixel(&img, 0, 0),
                want,
                &format!("{name} の {frame} 枚目の隅"),
            );
            let center = pixel(&img, SIZE.width / 2, SIZE.height / 2);
            assert_near(
                center,
                [GREEN.r, GREEN.g, GREEN.b, 1.0],
                &format!("{name} の {frame} 枚目の中央"),
            );
        }
        assert!(r.take_errors().is_empty());
    }
}

/// 青を返すシェーダー。シェーダーの種類も自前のパスをその場で提出する。
const BLUE_WGSL: &str =
    "fn mm_main(uv: vec2<f32>) -> vec4<f32> { return vec4<f32>(0.1, 0.2, 0.7, 1.0); }";

/// 赤の 2 秒（画布と同じ大きさの入れ子の単色）、青の 2 秒（シェーダー。先頭に長さ 2 秒のクロスディゾルブ）。
/// 出ていくカットが中間のテクスチャを 2 枚手放すので、入ってくるカットの画布とシェーダーの出力の両方が使い回しに当たる。
fn two_solid_cuts() -> mm_doc::Project {
    let red = comp("red", SIZE, 4, vec![layer("red-solid", solid(RED), 4)]);
    let nested =
        Node::new("composition", 1).with_param("composition", ParamValue::Text("red".into()));
    let mut second = Cut::new("b", shader(BLUE_WGSL), seconds(0), seconds(2));
    second.transition_in = Some(dissolve(seconds(2)));
    let seq = Sequence {
        id: Id::new("s"),
        cuts: vec![Cut::new("a", nested, seconds(0), seconds(2)), second],
        origin: None,
    };
    let node = Node::new("sequence", 1).with_param("sequence", ParamValue::Text("s".into()));
    let main = comp("main", SIZE, 4, vec![layer("seq", node, 4)]);
    project(vec![red, main], vec![seq])
}

#[test]
fn シーケンスの出ていくカットの画布を入ってくるカットが上書きしない() {
    let Some(ctx) = gpu() else { return };
    let p = two_solid_cuts();
    let mix = |a: f32, b: f32| (a + b) / 2.0;
    let want = [
        mix(RED.r, BLUE.r),
        mix(RED.g, BLUE.g),
        mix(RED.b, BLUE.b),
        1.0,
    ];
    for (name, mut r) in [
        ("キャッシュあり", renderer(&ctx)),
        (
            "キャッシュなし",
            renderer(&ctx).with_frame_cache(no_cache()),
        ),
    ] {
        for frame in 0..2 {
            let img = render_at(&mut r, &p, "main", seconds(2), SIZE);
            assert_near(pixel(&img, 3, 3), want, &format!("{name} の {frame} 枚目"));
        }
        assert!(r.take_errors().is_empty());
    }
}
