//! ノードの契約テスト。種類ごとに 1 行。GPU の要る `shader.wgsl` は `tests/gpu_kinds.rs`（実機の GPU）で確かめる。

use std::sync::Arc;

use mm_nodes_basic::{
    ColorAdjustKind, ColorCdlKind, ColorConvertKind, ColorLutKind, CompositionKind, DissolveKind,
    FootageKind, ImageKind, SequenceKind, SolidKind, TextKind,
};
use mm_testkit::decode::SampleMedia;
use mm_testkit::node::{contract, NodeFixture};
use mm_types::{ParamValue, Params, Rgba, RgbaImage, Size};

fn footage_fixture() -> NodeFixture {
    NodeFixture {
        footage: Some(SampleMedia::default()),
        ..NodeFixture::default()
    }
}

/// 色と透明が場所で変わる入力。効果の契約はこの画像を入力にして回す。
fn gradient(size: Size) -> Arc<RgbaImage> {
    let mut img = RgbaImage::transparent(size);
    for y in 0..size.height {
        for x in 0..size.width {
            let u = x as f32 / (size.width - 1) as f32;
            let v = y as f32 / (size.height - 1) as f32;
            let a = if y < 4 { 0.0 } else { 0.5 + 0.5 * u };
            img.set_pixel(x, y, Rgba::from_straight(u, v, 1.0 - u, a));
        }
    }
    Arc::new(img)
}

fn effect_fixture(params: Params) -> NodeFixture {
    let fx = NodeFixture::default();
    NodeFixture {
        inputs: vec![gradient(fx.size)],
        params,
        ..fx
    }
}

#[test]
fn solid_は契約を満たす() {
    let mut params = Params::new();
    params.set("color", ParamValue::Color(Rgba::new(0.2, 0.4, 0.6, 1.0)));
    contract(
        || Arc::new(SolidKind),
        &NodeFixture {
            params,
            ..NodeFixture::default()
        },
    );
}

#[test]
fn footage_は契約を満たす() {
    contract(|| Arc::new(FootageKind), &footage_fixture());
}

#[test]
fn image_は契約を満たす() {
    contract(|| Arc::new(ImageKind), &footage_fixture());
}

#[test]
fn text_は契約を満たす() {
    contract(|| Arc::new(TextKind::default()), &NodeFixture::default());
}

#[test]
fn composition_は契約を満たす() {
    contract(|| Arc::new(CompositionKind), &NodeFixture::default());
}

#[test]
fn sequence_は契約を満たす() {
    contract(|| Arc::new(SequenceKind), &NodeFixture::default());
}

#[test]
fn color_adjust_は既定値で恒等として契約を満たす() {
    contract(
        || Arc::new(ColorAdjustKind::default()),
        &effect_fixture(Params::new()),
    );
}

#[test]
fn color_adjust_は値を変えても契約を満たす() {
    let mut params = Params::new();
    params.set("brightness", ParamValue::Number(0.5));
    params.set("contrast", ParamValue::Number(1.4));
    params.set("saturation", ParamValue::Number(0.6));
    contract(
        || Arc::new(ColorAdjustKind::default()),
        &effect_fixture(params),
    );
}

#[test]
fn color_cdl_は契約を満たす() {
    let mut params = Params::new();
    params.set("slope", ParamValue::Vec3([1.2, 1.0, 0.8]));
    params.set("offset", ParamValue::Vec3([0.0, 0.02, 0.05]));
    params.set("power", ParamValue::Vec3([1.1, 1.0, 0.9]));
    params.set("saturation", ParamValue::Number(1.2));
    contract(
        || Arc::new(ColorCdlKind::default()),
        &effect_fixture(params),
    );
}

#[test]
fn color_lut_は契約を満たす() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/luts/invert.cube"
    );
    let mut params = Params::new();
    params.set("lut", ParamValue::AssetRef(mm_types::Id::new(path)));
    contract(
        || Arc::new(ColorLutKind::default()),
        &effect_fixture(params),
    );
}

#[test]
fn color_convert_は契約を満たす() {
    let mut params = Params::new();
    params.set("from", ParamValue::Text("lin_rec709".into()));
    params.set("to", ParamValue::Text("srgb".into()));
    contract(
        || Arc::new(ColorConvertKind::default()),
        &effect_fixture(params),
    );
}

#[test]
fn transition_dissolve_は契約を満たす() {
    let fx = NodeFixture::default();
    let from = gradient(fx.size);
    let to = Arc::new(RgbaImage::filled(fx.size, Rgba::new(0.0, 0.0, 0.5, 1.0)));
    contract(
        || Arc::new(DissolveKind::default()),
        &NodeFixture {
            inputs: vec![from, to],
            ..fx
        },
    );
}
