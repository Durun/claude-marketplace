//! GPU の経路（`WgpuGpu`）と CPU の経路（モックの GPU）が、色の種類で同じ画像を返すこと。
//! GPU が無い環境ではスキップし、`MM_REQUIRE_GPU` があれば失敗にする。

use std::sync::Arc;

use mm_color_lut::LutColor;
use mm_gpu::{GpuContext, GpuRequest, WgpuGpu};
use mm_node_api::{FrameCx, FrameRef, GpuHelper, NodeKind, Quality, Services};
use mm_nodes_basic::{
    ColorAdjustKind, ColorCdlKind, ColorConvertKind, ColorLutKind, DissolveKind, ShaderWgslKind,
};
use mm_testkit::fixtures::sample_services;
use mm_testkit::gpu::CpuGpu;
use mm_testkit::node::ConstInputs;
use mm_types::{MmError, ParamValue, Params, RationalTime, Rgba, RgbaImage, Size};

const SIZE: Size = Size::new(48, 27);
/// 入力の f16 への丸めと、式の f32 と f16 の差を見込んだ許容。
const TOLERANCE: f32 = 5e-3;

fn gpu() -> Option<Arc<GpuContext>> {
    match GpuContext::shared(&GpuRequest::default()) {
        Ok(ctx) => Some(ctx),
        Err(e) if std::env::var_os("MM_REQUIRE_GPU").is_some() => {
            panic!("MM_REQUIRE_GPU が有るのに GPU を作れない: {e}")
        }
        Err(e) => {
            eprintln!("GPU が無いのでスキップする: {e}");
            None
        }
    }
}

fn services() -> Services {
    Services {
        color: Arc::new(LutColor),
        ..sample_services()
    }
}

/// 色と透明が場所で変わる入力。
fn gradient() -> Arc<RgbaImage> {
    let mut img = RgbaImage::transparent(SIZE);
    for y in 0..SIZE.height {
        for x in 0..SIZE.width {
            let u = x as f32 / (SIZE.width - 1) as f32;
            let v = y as f32 / (SIZE.height - 1) as f32;
            let a = if y < 3 { 0.0 } else { 0.4 + 0.6 * u };
            img.set_pixel(x, y, Rgba::from_straight(u * 1.5, v, 1.0 - u, a));
        }
    }
    Arc::new(img)
}

fn render_with(
    kind: &dyn NodeKind,
    params: &Params,
    helper: &mut dyn GpuHelper,
    inputs: Vec<Arc<RgbaImage>>,
) -> Result<FrameRef, MmError> {
    let services = services();
    let mut inputs = ConstInputs(inputs);
    let mut cx = FrameCx {
        time: RationalTime::seconds(1),
        params,
        size: SIZE,
        region: SIZE.to_rect(),
        scale: 1.0,
        quality: Quality::Full,
        inputs: &mut inputs,
        footage: None,
        source_time: RationalTime::ZERO,
        gpu: helper,
        services: &services,
    };
    kind.render(&mut cx)
}

fn params_of(kind: &dyn NodeKind, set: &[(&str, ParamValue)]) -> Params {
    let mut params = kind.params().defaults();
    for (name, value) in set {
        params.set(*name, value.clone());
    }
    params
}

/// GPU と CPU の経路の最大の差を返す。GPU の結果は `WgpuGpu` からリードバックする。
fn gpu_cpu_diff(ctx: &Arc<GpuContext>, kind: &dyn NodeKind, params: &Params) -> f32 {
    let mut wgpu = WgpuGpu::new(ctx.clone());
    let frame = render_with(kind, params, &mut wgpu, vec![gradient()]).expect("GPU で描ける");
    assert!(
        matches!(frame, FrameRef::Gpu(_)),
        "GPU の経路はテクスチャを返す"
    );
    let on_gpu = wgpu.download(&frame).expect("リードバックできる");
    let mut cpu = CpuGpu::new();
    let frame = render_with(kind, params, &mut cpu, vec![gradient()]).expect("CPU で描ける");
    let on_cpu = cpu.download(&frame).expect("CPU の画像");
    on_gpu.max_diff(&on_cpu).expect("同じ大きさ")
}

fn check(kind: &dyn NodeKind, set: &[(&str, ParamValue)]) {
    let Some(ctx) = gpu() else { return };
    let diff = gpu_cpu_diff(&ctx, kind, &params_of(kind, set));
    assert!(diff <= TOLERANCE, "{} の GPU と CPU の差 {diff}", kind.id());
}

#[test]
fn color_adjust_は_gpu_と_cpu_で一致する() {
    check(
        &ColorAdjustKind::default(),
        &[
            ("brightness", ParamValue::Number(0.5)),
            ("contrast", ParamValue::Number(1.4)),
            ("saturation", ParamValue::Number(0.6)),
        ],
    );
}

#[test]
fn color_cdl_は_gpu_と_cpu_で一致する() {
    check(
        &ColorCdlKind::default(),
        &[
            ("slope", ParamValue::Vec3([1.2, 1.0, 0.8])),
            ("offset", ParamValue::Vec3([0.0, 0.02, 0.05])),
            ("power", ParamValue::Vec3([1.1, 1.0, 0.9])),
            ("saturation", ParamValue::Number(1.2)),
        ],
    );
}

#[test]
fn color_cdl_は_power_0_と_x_0_でも_gpu_が_nan_にならない() {
    // gradient の左端と上の行は R が 0。power 0 の R で pow(0, 0) を踏む。
    check(
        &ColorCdlKind::default(),
        &[("power", ParamValue::Vec3([0.0, 0.5, 1.0]))],
    );
}

#[test]
fn color_lut_は_gpu_と_cpu_で一致する() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/luts/invert.cube"
    );
    for space in ["lin_rec709", "srgb", "bt709"] {
        check(
            &ColorLutKind::default(),
            &[
                ("lut", ParamValue::AssetRef(mm_types::Id::new(path))),
                ("intensity", ParamValue::Number(0.75)),
                ("input_space", ParamValue::Choice(space.into())),
            ],
        );
    }
}

#[test]
fn color_convert_は_gpu_と_cpu_で一致する() {
    for (from, to) in [
        ("lin_rec709", "srgb"),
        ("lin_rec709", "bt2020"),
        ("bt709", "lin_rec709"),
    ] {
        check(
            &ColorConvertKind::default(),
            &[
                ("from", ParamValue::Text(from.into())),
                ("to", ParamValue::Text(to.into())),
            ],
        );
    }
}

#[test]
fn transition_dissolve_は_gpu_と_cpu_で一致する() {
    let Some(ctx) = gpu() else { return };
    let kind = DissolveKind::default();
    let params = params_of(&kind, &[("progress", ParamValue::Number(0.3))]);
    let other = Arc::new(RgbaImage::filled(SIZE, Rgba::new(0.0, 0.2, 0.0, 0.5)));
    let mut wgpu = WgpuGpu::new(ctx);
    let frame = render_with(&kind, &params, &mut wgpu, vec![gradient(), other.clone()])
        .expect("GPU で描ける");
    let on_gpu = wgpu.download(&frame).expect("リードバックできる");
    let mut cpu = CpuGpu::new();
    let frame = render_with(&kind, &params, &mut cpu, vec![gradient(), other]).expect("CPU");
    let diff = on_gpu
        .max_diff(&cpu.download(&frame).expect("CPU の画像"))
        .expect("同じ大きさ");
    assert!(diff <= TOLERANCE, "差 {diff}");
}

#[test]
fn shader_wgsl_は_gpu_が無ければ_unsupported() {
    let kind = ShaderWgslKind::default();
    let params = kind.params().defaults();
    let err = render_with(&kind, &params, &mut CpuGpu::new(), Vec::new()).unwrap_err();
    assert!(matches!(err, MmError::Unsupported(_)), "{err:?}");
}

#[test]
fn shader_wgsl_は入力の無い_uv_を描く() {
    let Some(ctx) = gpu() else { return };
    let kind = ShaderWgslKind::default();
    let params = kind.params().defaults();
    let mut wgpu = WgpuGpu::new(ctx);
    let frame = render_with(&kind, &params, &mut wgpu, Vec::new()).expect("描ける");
    let img = wgpu.download(&frame).expect("リードバックできる");
    assert_eq!(img.size(), SIZE);
    let corner = img.pixel(0, 0);
    let far = img.pixel(SIZE.width - 1, SIZE.height - 1);
    assert!(corner.r < 0.05 && far.r > 0.95, "u は左から右へ増える");
    assert!(corner.g < 0.05 && far.g > 0.95, "v は上から下へ増える");
}
