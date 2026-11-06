//! 色の種類: `color.adjust`（明るさ・コントラスト・彩度）と `color.cdl`（ASC CDL）。
//!
//! 入力は線形の premultiplied。アルファで割って straight にしてから処理し、また掛ける。処理は作業色空間（線形の光）の上で行う。
//! GPU は `FrameCx::gpu` が `WgpuGpu` のときの全画面パス、そうでなければ（モックの GPU）同じ式の CPU の経路。

use std::sync::Arc;

use mm_gpu::WgpuGpu;
use mm_node_api::{
    FrameCx, FrameRef, GpuHelper, GpuTexture, InputSchema, InputSpec, NodeKind, Traits,
};
use mm_types::{
    MmError, ParamSchema, ParamSpec, ParamType, ParamValue, Params, Result, Rgba, RgbaImage,
};

use crate::gpu_pass::{Pass, PassCache};
use crate::lut::Lut;
use crate::util::migrate_same_version;
use crate::KIND_VERSION;

/// Rec.709 の輝度係数。彩度の基準。
pub(crate) const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];
/// コントラストの支点。線形の光で中間のグレー（18%）。
const CONTRAST_PIVOT: f32 = 0.18;
/// `pow` の底に許す最小。0 と負の値で `log2` が発散しない。
const MIN_POW_BASE: f32 = 1e-10;

/// フラグメントシェーダーの共通部分。`mm_color` は種類ごとに足す。
const FRAGMENT_WGSL: &str = r"
struct U { a: vec4<f32>, b: vec4<f32>, c: vec4<f32>, d: vec4<f32> }

@fragment
fn mm_fs(in: MmVsOut) -> @location(0) vec4<f32> {
    let src = textureLoad(mm_t0, vec2<i32>(in.pos.xy), 0);
    var rgb = vec3<f32>(0.0);
    if (src.a > 0.0) {
        rgb = src.rgb / src.a;
    }
    return vec4<f32>(mm_color(rgb) * src.a, src.a);
}
";

/// 1 つの色の処理。GPU の WGSL と CPU の式は同じ変換を表す。
pub(crate) struct ColorPass<'a> {
    /// `fn mm_color(c: vec3<f32>) -> vec3<f32>` を含む WGSL（`u` の中身の使い方もここで決める）。
    pub wgsl: String,
    /// `U`（`a`、`b`、`c`、`d` の 4 つの vec4）の中身。
    pub uniform: [f32; 16],
    pub lut: Option<Arc<Lut>>,
    /// straight の RGB を処理する CPU の式。
    pub cpu: Box<dyn Fn([f32; 3]) -> [f32; 3] + 'a>,
}

fn input_schema() -> InputSchema {
    InputSchema {
        inputs: vec![InputSpec {
            name: "source".into(),
            optional: false,
        }],
    }
}

fn number(min: f64) -> ParamType {
    ParamType::Number {
        min: Some(min),
        max: None,
    }
}

/// 入力 0 のフレーム。
fn source_frame(cx: &mut FrameCx<'_>) -> Result<FrameRef> {
    let t = cx.time;
    cx.inputs.frame(0, t)
}

/// 入力に `pass` を掛けたフレーム。透明な入力は透明のまま返す。
pub(crate) fn apply(
    cx: &mut FrameCx<'_>,
    cache: &PassCache,
    pass: &ColorPass<'_>,
) -> Result<FrameRef> {
    let input = source_frame(cx)?;
    if matches!(input, FrameRef::Transparent { .. }) {
        return Ok(input);
    }
    if cx.gpu.as_any().is::<WgpuGpu>() {
        let gpu = cx
            .gpu
            .as_any()
            .downcast_mut::<WgpuGpu>()
            .expect("直前に型を確かめた");
        apply_gpu(gpu, cache, pass, &input)
    } else {
        apply_cpu(cx.gpu, pass, &input)
    }
}

fn apply_gpu(
    gpu: &mut WgpuGpu,
    cache: &PassCache,
    pass: &ColorPass<'_>,
    input: &FrameRef,
) -> Result<FrameRef> {
    let (id, uploaded) = match input {
        FrameRef::Gpu(tex) => (tex.id, false),
        FrameRef::Cpu(image) => match GpuHelper::upload(gpu, image)? {
            FrameRef::Gpu(tex) => (tex.id, true),
            _ => return Err(MmError::Internal("upload が Gpu を返さない".into())),
        },
        FrameRef::Transparent { .. } => {
            return Err(MmError::Internal("透明な入力はここに来ない".into()))
        }
    };
    let result = run_gpu(gpu, cache, pass, id, input.size());
    if uploaded {
        gpu.release_id(id);
    }
    result
}

fn run_gpu(
    gpu: &mut WgpuGpu,
    cache: &PassCache,
    pass: &ColorPass<'_>,
    input: u64,
    size: mm_types::Size,
) -> Result<FrameRef> {
    let lut = match &pass.lut {
        Some(lut) => Some(lut.texture(gpu.context())?),
        None => None,
    };
    let body = format!("{FRAGMENT_WGSL}\n{}", pass.wgsl);
    let id = cache.run(
        gpu,
        &Pass {
            body: &body,
            uniform: &uniform_bytes(&pass.uniform),
            inputs: &[input],
            lut: lut.as_deref(),
            size,
        },
    )?;
    Ok(FrameRef::Gpu(GpuTexture { id, size }))
}

fn uniform_bytes(values: &[f32; 16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn apply_cpu(gpu: &mut dyn GpuHelper, pass: &ColorPass<'_>, input: &FrameRef) -> Result<FrameRef> {
    let image = gpu.download(input)?;
    let mut out = RgbaImage::transparent(image.size());
    for y in 0..image.height() {
        for x in 0..image.width() {
            let px = image.pixel(x, y);
            if px.a <= 0.0 {
                continue;
            }
            let rgb = (pass.cpu)([px.r / px.a, px.g / px.a, px.b / px.a]);
            out.set_pixel(
                x,
                y,
                Rgba::new(rgb[0] * px.a, rgb[1] * px.a, rgb[2] * px.a, px.a),
            );
        }
    }
    Ok(FrameRef::cpu(out))
}

/// 0 の符号を 0 にする `signum`（WGSL の `sign` と同じ）。
fn sign(v: f32) -> f32 {
    if v == 0.0 {
        0.0
    } else {
        v.signum()
    }
}

fn saturate(v: [f32; 3], saturation: f32) -> [f32; 3] {
    let luma = v[0] * LUMA[0] + v[1] * LUMA[1] + v[2] * LUMA[2];
    v.map(|c| luma + (c - luma) * saturation)
}

/// 明るさ（露出の段数）、コントラスト（18% グレーを支点にしたべき）、彩度。
#[derive(Debug, Default)]
pub struct ColorAdjustKind {
    cache: PassCache,
}

/// `color.adjust` の係数。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Adjust {
    gain: f32,
    contrast: f32,
    saturation: f32,
}

impl Adjust {
    fn of(params: &Params) -> Self {
        Self {
            gain: (params.number_or("brightness", 0.0) as f32).exp2(),
            contrast: params.number_or("contrast", 1.0) as f32,
            saturation: params.number_or("saturation", 1.0) as f32,
        }
    }

    fn is_identity(&self) -> bool {
        self.gain == 1.0 && self.contrast == 1.0 && self.saturation == 1.0
    }

    fn cpu(&self, rgb: [f32; 3]) -> [f32; 3] {
        let v = rgb.map(|c| {
            let c = c * self.gain;
            let base = (c.abs() / CONTRAST_PIVOT).max(MIN_POW_BASE);
            sign(c) * (self.contrast * base.log2()).exp2() * CONTRAST_PIVOT
        });
        saturate(v, self.saturation)
    }
}

const ADJUST_WGSL: &str = r"
fn mm_color(c: vec3<f32>) -> vec3<f32> {
    let v0 = c * u.a.x;
    let base = max(abs(v0) / 0.18, vec3<f32>(1e-10));
    let v = sign(v0) * exp2(u.a.y * log2(base)) * 0.18;
    let luma = dot(v, vec3<f32>(0.2126, 0.7152, 0.0722));
    return vec3<f32>(luma) + (v - vec3<f32>(luma)) * u.a.z;
}
";

impl NodeKind for ColorAdjustKind {
    fn id(&self) -> &'static str {
        "color.adjust"
    }

    fn version(&self) -> u32 {
        KIND_VERSION
    }

    fn params(&self) -> ParamSchema {
        ParamSchema::new(vec![
            ParamSpec::new(
                "brightness",
                ParamType::Number {
                    min: None,
                    max: None,
                },
                ParamValue::Number(0.0),
            ),
            ParamSpec::new("contrast", number(0.0), ParamValue::Number(1.0)),
            ParamSpec::new("saturation", number(0.0), ParamValue::Number(1.0)),
        ])
    }

    fn inputs(&self) -> InputSchema {
        input_schema()
    }

    fn traits(&self) -> Traits {
        Traits::PIXEL
    }

    fn migrate(&self, old: serde_json::Value, from: u32) -> Result<Params> {
        migrate_same_version(self, old, from)
    }

    fn is_identity(&self, cx: &FrameCx<'_>) -> bool {
        Adjust::of(cx.params).is_identity()
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        let adjust = Adjust::of(cx.params);
        if adjust.is_identity() {
            return source_frame(cx);
        }
        let mut uniform = [0.0; 16];
        uniform[..3].copy_from_slice(&[adjust.gain, adjust.contrast, adjust.saturation]);
        let pass = ColorPass {
            wgsl: ADJUST_WGSL.into(),
            uniform,
            lut: None,
            cpu: Box::new(move |rgb| adjust.cpu(rgb)),
        };
        apply(cx, &self.cache, &pass)
    }
}

/// ASC CDL。`out = clamp(in * slope + offset) ^ power` の後に Rec.709 の輝度係数で彩度を掛ける。
/// 線形の光の上で掛ける（CDL の定義は符号化された値の上だが、作業色空間で処理する決まり）。
#[derive(Debug, Default)]
pub struct ColorCdlKind {
    cache: PassCache,
}

/// `color.cdl` の 10 個の数。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Cdl {
    slope: [f32; 3],
    offset: [f32; 3],
    power: [f32; 3],
    saturation: f32,
}

fn vec3_or(params: &Params, name: &str, fallback: f32) -> [f32; 3] {
    match params.get(name) {
        Some(ParamValue::Vec3(v)) => v.map(|c| c as f32),
        _ => [fallback; 3],
    }
}

impl Cdl {
    fn of(params: &Params) -> Self {
        Self {
            slope: vec3_or(params, "slope", 1.0),
            offset: vec3_or(params, "offset", 0.0),
            power: vec3_or(params, "power", 1.0),
            saturation: params.number_or("saturation", 1.0) as f32,
        }
    }

    fn is_identity(&self) -> bool {
        self.slope == [1.0; 3]
            && self.offset == [0.0; 3]
            && self.power == [1.0; 3]
            && self.saturation == 1.0
    }

    fn cpu(&self, rgb: [f32; 3]) -> [f32; 3] {
        let v = std::array::from_fn(|i| {
            (rgb[i] * self.slope[i] + self.offset[i])
                .max(0.0)
                .powf(self.power[i])
        });
        saturate(v, self.saturation)
    }
}

const CDL_WGSL: &str = r"
fn mm_color(c: vec3<f32>) -> vec3<f32> {
    let v = pow(max(c * u.a.xyz + u.b.xyz, vec3<f32>(0.0)), u.c.xyz);
    let luma = dot(v, vec3<f32>(0.2126, 0.7152, 0.0722));
    return vec3<f32>(luma) + (v - vec3<f32>(luma)) * u.d.x;
}
";

impl NodeKind for ColorCdlKind {
    fn id(&self) -> &'static str {
        "color.cdl"
    }

    fn version(&self) -> u32 {
        KIND_VERSION
    }

    fn params(&self) -> ParamSchema {
        ParamSchema::new(vec![
            ParamSpec::new("slope", ParamType::Vec3, ParamValue::Vec3([1.0; 3])),
            ParamSpec::new("offset", ParamType::Vec3, ParamValue::Vec3([0.0; 3])),
            ParamSpec::new("power", ParamType::Vec3, ParamValue::Vec3([1.0; 3])),
            ParamSpec::new("saturation", number(0.0), ParamValue::Number(1.0)),
        ])
    }

    fn inputs(&self) -> InputSchema {
        input_schema()
    }

    fn traits(&self) -> Traits {
        Traits::PIXEL
    }

    fn migrate(&self, old: serde_json::Value, from: u32) -> Result<Params> {
        migrate_same_version(self, old, from)
    }

    fn is_identity(&self, cx: &FrameCx<'_>) -> bool {
        Cdl::of(cx.params).is_identity()
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        let cdl = Cdl::of(cx.params);
        if cdl.is_identity() {
            return source_frame(cx);
        }
        let mut uniform = [0.0; 16];
        uniform[0..3].copy_from_slice(&cdl.slope);
        uniform[4..7].copy_from_slice(&cdl.offset);
        uniform[8..11].copy_from_slice(&cdl.power);
        uniform[12] = cdl.saturation;
        let pass = ColorPass {
            wgsl: CDL_WGSL.into(),
            uniform,
            lut: None,
            cpu: Box::new(move |rgb| cdl.cpu(rgb)),
        };
        apply(cx, &self.cache, &pass)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn コントラスト_1_と明るさ_0_は恒等() {
        let adjust = Adjust::of(&Params::new());
        assert!(adjust.is_identity());
        let out = adjust.cpu([0.18, 0.5, 2.0]);
        for (a, b) in out.iter().zip([0.18, 0.5, 2.0]) {
            assert!((a - b).abs() < 1e-5, "{out:?}");
        }
    }

    #[test]
    fn 明るさ_1_は露出を_1_段上げる() {
        let mut params = Params::new();
        params.set("brightness", ParamValue::Number(1.0));
        let out = Adjust::of(&params).cpu([0.1, 0.2, 0.4]);
        for (a, b) in out.iter().zip([0.2, 0.4, 0.8]) {
            assert!((a - b).abs() < 1e-5, "{out:?}");
        }
    }

    #[test]
    fn コントラストは中間グレーを動かさない() {
        let mut params = Params::new();
        params.set("contrast", ParamValue::Number(1.5));
        let out = Adjust::of(&params).cpu([0.18; 3]);
        assert!(out.iter().all(|c| (c - 0.18).abs() < 1e-5), "{out:?}");
        let bright = Adjust::of(&params).cpu([0.5; 3]);
        assert!(bright[0] > 0.5, "明るい側はさらに明るい");
    }

    #[test]
    fn 彩度_0_は輝度のグレー() {
        let mut params = Params::new();
        params.set("saturation", ParamValue::Number(0.0));
        let out = Adjust::of(&params).cpu([1.0, 0.0, 0.0]);
        assert!((out[0] - 0.2126).abs() < 1e-5 && out[0] == out[1] && out[1] == out[2]);
    }

    #[test]
    fn cdl_の式は_mm_color_lut_と一致する() {
        let mut params = Params::new();
        params.set("slope", ParamValue::Vec3([1.2, 0.9, 1.1]));
        params.set("offset", ParamValue::Vec3([0.02, -0.01, 0.0]));
        params.set("power", ParamValue::Vec3([1.1, 1.0, 0.9]));
        params.set("saturation", ParamValue::Number(0.8));
        let ours = Cdl::of(&params);
        let theirs = mm_color_lut::Cdl {
            slope: ours.slope,
            offset: ours.offset,
            power: ours.power,
            saturation: ours.saturation,
        };
        for rgb in [[0.1, 0.5, 0.9], [0.0, 0.3, 1.5]] {
            let (a, b) = (ours.cpu(rgb), theirs.apply(rgb));
            for (x, y) in a.iter().zip(b) {
                assert!((x - y).abs() < 1e-6, "{rgb:?}: {a:?} vs {b:?}");
            }
        }
    }
}
