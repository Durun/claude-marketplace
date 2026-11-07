//! 期待画像のテスト。各種類のテストパターンを `fixtures/golden/<バックエンド>/` の PNG と比べる（`mm_testkit::golden`）。
//! 更新は `cargo xtask golden-update`。GPU が無い環境と、期待画像のフォルダーが無いバックエンドではスキップする。
//! `MM_GOLDEN_BACKEND=warp` のときは WARP のアダプターを選ぶ（CI の WARP と、手元で WARP の期待画像を作るとき）。

use crate::common;

use std::path::Path;
use std::sync::Arc;

use common::{
    comp, dissolve, effect, gpu, layer, project, render_at, renderer, seconds, shader, solid,
};
use mm_doc::{Asset, Cut, Node, Project, Sequence};
use mm_gpu::enumerate_adapters;
use mm_render::{GpuContext, GpuRequest};
use mm_testkit::decode::SampleMedia;
use mm_testkit::fixtures::{invert_lut_asset, INVERT_LUT_ID, SAMPLE_ASSET_PATH};
use mm_testkit::golden::{backend_name, Golden, BACKEND_ENV, GOLDEN_SIZE};
use mm_types::{ColorSpace, Id, ParamValue, RationalTime, Rgba};

/// テストパターン: 上段に 6 色のパッチ、中段に線形の灰色のランプ、下段に肌色と暗部のパッチ。`p0` は動く縦線の明るさ、
/// 縦線は `u.time` 秒で右へ 1 秒に 1/4 画面進む。
const TEST_CARD: &str = r"
fn mm_main(uv: vec2<f32>) -> vec4<f32> {
    var c = vec3<f32>(0.0);
    if (uv.y < 0.5) {
        let i = floor(uv.x * 6.0);
        if (i < 1.0) { c = vec3<f32>(0.8, 0.05, 0.05); }
        else if (i < 2.0) { c = vec3<f32>(0.05, 0.6, 0.05); }
        else if (i < 3.0) { c = vec3<f32>(0.05, 0.05, 0.8); }
        else if (i < 4.0) { c = vec3<f32>(0.05, 0.6, 0.6); }
        else if (i < 5.0) { c = vec3<f32>(0.7, 0.05, 0.7); }
        else { c = vec3<f32>(0.7, 0.6, 0.05); }
    } else if (uv.y < 0.75) {
        c = vec3<f32>(uv.x);
    } else if (uv.x < 0.5) {
        c = vec3<f32>(0.45, 0.28, 0.22);
    } else {
        c = vec3<f32>(0.03, 0.04, 0.06);
    }
    if (abs(uv.x - fract(u.time * 0.25)) < 0.01) {
        c = vec3<f32>(u.p0.x);
    }
    return vec4<f32>(c, 1.0);
}
";

/// 切り替えが見やすい別のテストパターン: 斜めの縞。`p0` で色味、`p1` で縞の向き（1 か -1）を変える。
const STRIPES: &str = r"
fn mm_main(uv: vec2<f32>) -> vec4<f32> {
    let s = step(0.5, fract((uv.x + uv.y * u.p0.y) * 6.0));
    return vec4<f32>(mix(vec3<f32>(0.02, 0.02, 0.1), vec3<f32>(0.9, 0.5, u.p0.x), s), 1.0);
}
";

fn card() -> Node {
    shader(TEST_CARD).with_param("p0", ParamValue::Number(0.9))
}

fn main_comp(layers: Vec<mm_doc::Layer>) -> mm_doc::Composition {
    comp("main", GOLDEN_SIZE, 4, layers)
}

fn effect_on_card(node: Node) -> Project {
    let mut l = layer("card", card(), 4);
    l.effects.push(effect("e", node));
    project(vec![main_comp(vec![l])], vec![])
}

fn clip_project(layers: Vec<mm_doc::Layer>) -> Project {
    let media = SampleMedia::default();
    let mut p = project(vec![main_comp(layers)], vec![]);
    p.assets.push(Asset {
        id: Id::new("clip"),
        path: SAMPLE_ASSET_PATH.into(),
        content_hash: "sample0000000000".into(),
        color_space: ColorSpace::bt709(),
        duration: media.duration,
        fps: Some(media.fps),
        timebase_offset: RationalTime::ZERO,
    });
    p
}

/// 青い縞の 2 秒、橙の縞の 2 秒（先頭に長さ 1 秒のクロスディゾルブ）。
fn two_cuts() -> Project {
    let cut = |id: &str, p0: f64, direction: f64| {
        let node = shader(STRIPES)
            .with_param("p0", ParamValue::Number(p0))
            .with_param("p1", ParamValue::Number(direction));
        Cut::new(id, node, seconds(0), seconds(2))
    };
    let mut second = cut("b", 0.9, -1.0);
    second.transition_in = Some(dissolve(seconds(1)));
    let seq = Sequence {
        id: Id::new("s"),
        cuts: vec![cut("a", 0.1, 1.0), second],
        origin: None,
    };
    let node = Node::new("sequence", 1).with_param("sequence", ParamValue::Text("s".into()));
    project(vec![main_comp(vec![layer("seq", node, 4)])], vec![seq])
}

/// テストパターンを描いて、期待画像 `name` と比べる。
/// WARP のアダプターの名前（D3D12 のソフトウェアの GPU）。
const WARP_ADAPTER: &str = "Microsoft Basic Render Driver";

/// 期待画像を描く GPU。`MM_GOLDEN_BACKEND=warp` のときは、実機の GPU がある PC でも WARP を選ぶ
/// （`cargo xtask golden-update --backend warp` で WARP の期待画像を作れるように）。
fn golden_gpu() -> Option<Arc<GpuContext>> {
    if std::env::var(BACKEND_ENV).as_deref() != Ok("warp") {
        return gpu();
    }
    let adapter = enumerate_adapters()
        .into_iter()
        .find(|a| a.name == WARP_ADAPTER)
        .expect("WARP のアダプターが無い");
    let request = GpuRequest {
        adapter: Some(adapter),
        backends: wgpu::Backends::DX12,
        ..GpuRequest::default()
    };
    Some(GpuContext::shared(&request).expect("WARP のコンテキストを作れる"))
}

fn check(name: &str, project: &Project, t: RationalTime) {
    let Some(ctx) = golden_gpu() else { return };
    let backend = backend_name(&format!("{:?}", ctx.backend()), ctx.adapter_name());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden");
    let actual = Path::new(env!("CARGO_TARGET_TMPDIR")).join("golden-actual");
    let Some(golden) = Golden::open(&root, &backend, &actual) else {
        return;
    };
    let mut r = renderer(&ctx);
    let img = render_at(&mut r, project, "main", t, GOLDEN_SIZE);
    let errors = r.take_errors();
    assert!(errors.is_empty(), "どのレイヤーも描けている: {errors:?}");
    if let Err(message) = golden.check(name, &img) {
        panic!("{message}");
    }
}

#[test]
fn shader_wgsl_の期待画像() {
    let p = project(vec![main_comp(vec![layer("card", card(), 4)])], vec![]);
    check("shader-wgsl", &p, seconds(1));
}

#[test]
fn color_adjust_の期待画像() {
    let node = Node::new("color.adjust", 1)
        .with_param("brightness", ParamValue::Number(0.5))
        .with_param("contrast", ParamValue::Number(1.3))
        .with_param("saturation", ParamValue::Number(0.5));
    check("color-adjust", &effect_on_card(node), seconds(1));
}

#[test]
fn color_cdl_の期待画像() {
    let node = Node::new("color.cdl", 1)
        .with_param("slope", ParamValue::Vec3([1.2, 1.0, 0.8]))
        .with_param("offset", ParamValue::Vec3([0.02, 0.0, 0.0]))
        .with_param("power", ParamValue::Vec3([1.0, 1.0, 1.2]))
        .with_param("saturation", ParamValue::Number(1.1));
    check("color-cdl", &effect_on_card(node), seconds(1));
}

#[test]
fn color_lut_の期待画像() {
    let node = Node::new("color.lut", 1)
        .with_param("lut", ParamValue::AssetRef(Id::new(INVERT_LUT_ID)))
        .with_param("input_space", ParamValue::Choice("srgb".into()));
    let mut p = effect_on_card(node);
    p.assets.push(invert_lut_asset());
    check("color-lut", &p, seconds(1));
}

#[test]
fn color_convert_の期待画像() {
    let node = Node::new("color.convert", 1)
        .with_param("from", ParamValue::Text("lin_rec709".into()))
        .with_param("to", ParamValue::Text("srgb".into()));
    check("color-convert", &effect_on_card(node), seconds(1));
}

#[test]
fn 入れ子に掛けた効果の期待画像() {
    let inner = comp("inner", GOLDEN_SIZE, 4, vec![layer("card", card(), 4)]);
    let mut nested = layer(
        "nested",
        Node::new("composition", 1).with_param("composition", ParamValue::Text("inner".into())),
        4,
    );
    nested.transform.scale = mm_doc::Param::constant(ParamValue::Number(0.6));
    nested.effects.push(effect(
        "e",
        Node::new("color.adjust", 1).with_param("saturation", ParamValue::Number(0.2)),
    ));
    let bg = layer("bg", solid(Rgba::new(0.02, 0.1, 0.12, 1.0)), 4);
    let p = project(vec![inner, main_comp(vec![bg, nested])], vec![]);
    check("color-nested", &p, seconds(2));
}

#[test]
fn sequence_のカットの期待画像() {
    check(
        "sequence-cut",
        &two_cuts(),
        RationalTime::new(7, 2).unwrap(),
    );
}

#[test]
fn sequence_のクロスディゾルブの期待画像() {
    check(
        "sequence-dissolve",
        &two_cuts(),
        RationalTime::new(9, 4).unwrap(),
    );
}

#[test]
fn text_の期待画像() {
    let text = Node::new("text", 1)
        .with_param("text", ParamValue::Text("Golden".into()))
        .with_param("size", ParamValue::Number(48.0))
        .with_param("position", ParamValue::Vec2([20.0, 60.0]));
    let bg = layer("bg", solid(Rgba::new(0.05, 0.1, 0.3, 1.0)), 4);
    let p = project(vec![main_comp(vec![bg, layer("t", text, 4)])], vec![]);
    check("text", &p, seconds(0));
}

#[test]
fn image_の期待画像() {
    let node = Node::new("image", 1).with_param("asset", ParamValue::AssetRef(Id::new("clip")));
    let bg = layer("bg", solid(Rgba::new(0.1, 0.1, 0.1, 1.0)), 4);
    let mut img = layer("img", node, 4);
    img.transform.scale = mm_doc::Param::constant(ParamValue::Number(0.5));
    img.transform.rotation = mm_doc::Param::constant(ParamValue::Number(10.0));
    check("image", &clip_project(vec![bg, img]), seconds(1));
}

#[test]
fn 切り抜きと角丸のマスクの期待画像() {
    let num = |v: f64| mm_doc::Param::constant(ParamValue::Number(v));
    let bg = layer("bg", solid(Rgba::new(0.1, 0.1, 0.1, 1.0)), 4);
    let mut card = layer("card", card(), 4);
    card.crop.left = Some(num(30.0));
    card.crop.bottom = Some(num(20.0));
    let mut mask = mm_doc::Mask::rect(
        "m",
        mm_doc::MaskRect::new([20.0, 20.0], [240.0, 150.0], 28.0),
    );
    mask.feather = num(3.0);
    card.masks.push(mask);
    card.transform.scale = num(0.8);
    card.transform.rotation = num(6.0);
    let p = project(vec![main_comp(vec![bg, card])], vec![]);
    check("mask-crop", &p, seconds(1));
}
