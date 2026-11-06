//! 色の種類: `color.lut`（`.cube` の 3D LUT）と `color.convert`（`mm-color-api` の色空間の変換）。
//!
//! どちらも入力は作業色空間（線形の光）。`color.lut` は LUT の入力の色空間（`input_space`）に符号化してから引き、
//! 線形に戻して `intensity` で元と混ぜる。`color.convert` は入力の値を `from` の符号化値として読み、`to` の符号化値にする。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use mm_color_api::{ColorTransform, TransformProgram};
use mm_node_api::{FrameCx, FrameRef, InputSchema, InputSpec, NodeKind, Traits};
use mm_types::{
    AssetId, ColorSpace, MmError, ParamSchema, ParamSpec, ParamType, ParamValue, Params, Result,
};

use crate::color::{apply, ColorPass};
use crate::gpu_pass::PassCache;
use crate::lut::{Lut, LUT_WGSL};
use crate::util::migrate_same_version;
use crate::KIND_VERSION;

/// LUT の入力の色空間の名前。`mm-color-api` の名前の表のうち、LUT でよく使う 3 つ。
const SPACES: [&str; 3] = ["lin_rec709", "srgb", "bt709"];
/// LUT の `.cube` の素材（`AssetRef`）を持つパラメータの名前。
pub const LUT_PARAM: &str = "lut";
/// 線形の光の折れ点（sRGB）。
const SRGB_LINEAR_CUT: f32 = 0.003_130_8;
const SRGB_ENCODED_CUT: f32 = 0.040_45;
/// Rec.1886 の表示の式のべき。
const GAMMA: f32 = 2.4;

fn input_schema() -> InputSchema {
    InputSchema {
        inputs: vec![InputSpec {
            name: "source".into(),
            optional: false,
        }],
    }
}

fn source_frame(cx: &mut FrameCx<'_>) -> Result<FrameRef> {
    let t = cx.time;
    cx.inputs.frame(0, t)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Linear,
    Srgb,
    Bt709,
}

impl Encoding {
    fn of(params: &Params) -> Self {
        match params.get("input_space").and_then(ParamValue::as_text) {
            Some("srgb") => Self::Srgb,
            Some("lin_rec709") => Self::Linear,
            _ => Self::Bt709,
        }
    }

    /// WGSL の `u.d.x` に入れる番号。
    fn code(self) -> f32 {
        match self {
            Self::Linear => 0.0,
            Self::Srgb => 1.0,
            Self::Bt709 => 2.0,
        }
    }

    fn encode(self, v: f32) -> f32 {
        let a = v.abs();
        let e = match self {
            Self::Linear => a,
            Self::Srgb if a <= SRGB_LINEAR_CUT => 12.92 * a,
            Self::Srgb => 1.055 * a.powf(1.0 / GAMMA) - 0.055,
            Self::Bt709 => a.powf(1.0 / GAMMA),
        };
        if v < 0.0 {
            -e
        } else {
            e
        }
    }

    fn decode(self, v: f32) -> f32 {
        let a = v.abs();
        let l = match self {
            Self::Linear => a,
            Self::Srgb if a <= SRGB_ENCODED_CUT => a / 12.92,
            Self::Srgb => ((a + 0.055) / 1.055).powf(GAMMA),
            Self::Bt709 => a.powf(GAMMA),
        };
        if v < 0.0 {
            -l
        } else {
            l
        }
    }
}

/// 読んだ LUT と、読んだ時のファイルの更新時刻。
struct Loaded {
    path: PathBuf,
    modified: Option<SystemTime>,
    lut: Arc<Lut>,
}

/// `.cube` の 3D LUT を当てる。
#[derive(Default)]
pub struct ColorLutKind {
    cache: PassCache,
    loaded: Mutex<Option<Loaded>>,
}

impl std::fmt::Debug for ColorLutKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ColorLutKind")
    }
}

impl ColorLutKind {
    /// `path` の LUT。更新時刻が同じなら読み直さない。
    fn load(&self, path: &Path) -> Result<Arc<Lut>> {
        let path = path.to_path_buf();
        let modified = std::fs::metadata(&path)?.modified().ok();
        let mut guard = self
            .loaded
            .lock()
            .map_err(|_| MmError::Internal("LUT の取り置きが壊れた".into()))?;
        if let Some(l) = guard
            .as_ref()
            .filter(|l| l.path == path && l.modified == modified)
        {
            return Ok(l.lut.clone());
        }
        let lut = Arc::new(Lut::parse_cube(&std::fs::read_to_string(&path)?)?);
        *guard = Some(Loaded {
            path,
            modified,
            lut: lut.clone(),
        });
        Ok(lut)
    }
}

const LUT_BODY_WGSL: &str = r"
fn mm_encode(c: vec3<f32>) -> vec3<f32> {
    let a = abs(c);
    if (u.d.x == 1.0) {
        return sign(c) * select(1.055 * pow(a, vec3<f32>(1.0 / 2.4)) - 0.055, 12.92 * a, a <= vec3<f32>(0.0031308));
    }
    if (u.d.x == 2.0) {
        return sign(c) * pow(a, vec3<f32>(1.0 / 2.4));
    }
    return c;
}

fn mm_decode(c: vec3<f32>) -> vec3<f32> {
    let a = abs(c);
    if (u.d.x == 1.0) {
        return sign(c) * select(pow((a + 0.055) / 1.055, vec3<f32>(2.4)), a / 12.92, a <= vec3<f32>(0.04045));
    }
    if (u.d.x == 2.0) {
        return sign(c) * pow(a, vec3<f32>(2.4));
    }
    return c;
}

fn mm_color(c: vec3<f32>) -> vec3<f32> {
    let span = max(u.c.xyz - u.b.xyz, vec3<f32>(1e-6));
    let looked = mm_decode(mm_lut_sample((mm_encode(c) - u.b.xyz) / span));
    return mix(c, looked, u.a.x);
}
";

impl NodeKind for ColorLutKind {
    fn id(&self) -> &'static str {
        "color.lut"
    }

    fn version(&self) -> u32 {
        KIND_VERSION
    }

    fn params(&self) -> ParamSchema {
        ParamSchema::new(vec![
            ParamSpec::new(
                LUT_PARAM,
                ParamType::AssetRef,
                ParamValue::AssetRef(AssetId::new("")),
            ),
            ParamSpec::new(
                "intensity",
                ParamType::Number {
                    min: Some(0.0),
                    max: Some(1.0),
                },
                ParamValue::Number(1.0),
            ),
            ParamSpec::new(
                "input_space",
                ParamType::Choice {
                    options: SPACES.map(String::from).to_vec(),
                },
                ParamValue::Choice("bt709".into()),
            ),
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
        lut_is_identity(cx.params)
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        if lut_is_identity(cx.params) {
            return source_frame(cx);
        }
        let lut = self.load(&lut_path(cx)?)?;
        let intensity = cx.params.number_or("intensity", 1.0).clamp(0.0, 1.0) as f32;
        let encoding = Encoding::of(cx.params);
        let mut uniform = [0.0; 16];
        uniform[0] = intensity;
        uniform[4..7].copy_from_slice(&lut.domain_min);
        uniform[8..11].copy_from_slice(&lut.domain_max);
        uniform[12] = encoding.code();
        let cpu_lut = lut.clone();
        let pass = ColorPass {
            wgsl: format!("{LUT_WGSL}\n{LUT_BODY_WGSL}"),
            uniform,
            lut: Some(lut),
            cpu: Box::new(move |rgb| {
                let enc = rgb.map(|c| encoding.encode(c));
                let looked = cpu_lut.sample(enc).map(|c| encoding.decode(c));
                std::array::from_fn(|i| rgb[i] + (looked[i] - rgb[i]) * intensity)
            }),
        };
        apply(cx, &self.cache, &pass)
    }
}

/// LUT の素材。`AssetRef` が無いか空なら `None`。
fn lut_asset(params: &Params) -> Option<&AssetId> {
    params
        .get(LUT_PARAM)
        .and_then(ParamValue::as_asset)
        .filter(|id| !id.as_str().is_empty())
}

/// LUT の素材のファイル。素材をパスに直すのは、プロジェクトのデータを持つ呼び手（`InputFrames::asset_path`）。
fn lut_path(cx: &FrameCx<'_>) -> Result<PathBuf> {
    let id =
        lut_asset(cx.params).ok_or_else(|| MmError::InvalidArgument("LUT の素材が無い".into()))?;
    cx.inputs
        .asset_path(id)
        .ok_or_else(|| MmError::NotFound(format!("LUT の素材 {id}")))
}

/// 素材が無いか、強さが 0 なら何もしない。
fn lut_is_identity(params: &Params) -> bool {
    lut_asset(params).is_none() || params.number_or("intensity", 1.0) <= 0.0
}

/// `ColorTransform::program` の結果をシェーダーの形にしたもの。
struct Program {
    wgsl: String,
    lut: Option<Arc<Lut>>,
}

/// 直近に作った変換。組が変わらない間は `program` を呼び直さない。
struct Converted {
    from: ColorSpace,
    to: ColorSpace,
    program: Option<Arc<Program>>,
}

/// 色空間の変換。入力の値を `from` の符号化値として読み、`to` の符号化値にして返す。
#[derive(Default)]
pub struct ColorConvertKind {
    cache: PassCache,
    converted: Mutex<Option<Converted>>,
}

impl std::fmt::Debug for ColorConvertKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ColorConvertKind")
    }
}

fn space(params: &Params, name: &str) -> ColorSpace {
    ColorSpace(
        params
            .get(name)
            .and_then(ParamValue::as_text)
            .unwrap_or("lin_rec709")
            .to_string(),
    )
}

fn shader_program(program: TransformProgram) -> Result<Option<Program>> {
    Ok(match program {
        TransformProgram::Identity => None,
        TransformProgram::Wgsl { source } => Some(Program {
            wgsl: format!("{source}\nfn mm_color(c: vec3<f32>) -> vec3<f32> {{ return mm_color_transform(c); }}\n"),
            lut: None,
        }),
        TransformProgram::Lut3d { size, data } => Some(Program {
            wgsl: format!(
                "{LUT_WGSL}\nfn mm_color(c: vec3<f32>) -> vec3<f32> {{ return mm_lut_sample(c); }}\n"
            ),
            lut: Some(Arc::new(Lut::new(size, data, [0.0; 3], [1.0; 3])?)),
        }),
    })
}

impl ColorConvertKind {
    fn program(
        &self,
        color: &dyn ColorTransform,
        from: &ColorSpace,
        to: &ColorSpace,
    ) -> Result<Option<Arc<Program>>> {
        let mut guard = self
            .converted
            .lock()
            .map_err(|_| MmError::Internal("変換の取り置きが壊れた".into()))?;
        if let Some(c) = guard.as_ref().filter(|c| &c.from == from && &c.to == to) {
            return Ok(c.program.clone());
        }
        let program = shader_program(color.program(from, to)?)?.map(Arc::new);
        *guard = Some(Converted {
            from: from.clone(),
            to: to.clone(),
            program: program.clone(),
        });
        Ok(program)
    }
}

impl NodeKind for ColorConvertKind {
    fn id(&self) -> &'static str {
        "color.convert"
    }

    fn version(&self) -> u32 {
        KIND_VERSION
    }

    fn params(&self) -> ParamSchema {
        let name = || ParamValue::Text("lin_rec709".into());
        ParamSchema::new(vec![
            ParamSpec::new("from", ParamType::Text, name()),
            ParamSpec::new("to", ParamType::Text, name()),
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
        space(cx.params, "from") == space(cx.params, "to")
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        let (from, to) = (space(cx.params, "from"), space(cx.params, "to"));
        if from == to {
            return source_frame(cx);
        }
        let color = cx.services.color.clone();
        let Some(program) = self.program(color.as_ref(), &from, &to)? else {
            return source_frame(cx);
        };
        // 知らない色空間はここで `Err` にし、画素ごとの式は失敗しない前提にする。
        color.apply(&from, &to, [0.0; 3])?;
        let pass = ColorPass {
            wgsl: program.wgsl.clone(),
            uniform: [0.0; 16],
            lut: program.lut.clone(),
            cpu: Box::new(move |rgb| color.apply(&from, &to, rgb).unwrap_or(rgb)),
        };
        apply(cx, &self.cache, &pass)
    }
}
