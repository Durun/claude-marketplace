//! `ColorTransform` の OCIO 実装。GPU の形は、シェーダーがテクスチャも動的な値も持たなければ
//! naga で WGSL にし、持つ（または naga が読めない）ときは CPU の変換を 3D LUT に焼く。

use std::collections::HashMap;
use std::sync::Mutex;

use mm_color_api::{ColorTransform, TransformProgram};
use mm_color_lut::bake_lut3d;
use mm_types::{ColorSpace, MmError, Result};
use tracing::info_span;

use crate::ffi::{self, Config, Processor};
use crate::glsl::glsl_to_wgsl;

/// 既定の設定。OCIO の組み込みの studio 設定。`ocio://default`（CG 設定）には
/// `bt709_oetf` に当てるカメラの Rec.709（`Camera Rec.709`）が無く、studio 設定には ACES 2.0 の
/// 表示変換もそろっている。
pub const DEFAULT_CONFIG: &str = "ocio://studio-config-latest";
/// `to` にこの名前を渡すと、設定の既定の display と view で見る変換になる。
/// ACES 2.0 のトーンマップを含むので逆変換が無く、`known_spaces` には出さない。
pub const DISPLAY_SPACE: &str = "display";
/// 実装の名前。
pub const OCIO_ID: &str = "ocio";
/// OCIO の GLSL の入口の関数名。
const ENTRY: &str = "OCIOConvert";
/// 焼く 3D LUT の一辺の既定。
const DEFAULT_LUT_SIZE: u32 = 33;

/// mm の色空間名と OCIO の名前の対応。ここに無い名前は OCIO の名前としてそのまま引く。
const ALIASES: [(&str, &str); 4] = [
    ("lin_rec709", "Linear Rec.709 (sRGB)"),
    ("srgb", "sRGB Encoded Rec.709 (sRGB)"),
    // 映像の Rec.709 は表示側の Rec.1886（ガンマ 2.4）で見るので、その色空間に当てる。
    ("bt709", "Rec.1886 Rec.709 - Display"),
    // カメラの OETF（シーン参照）。組み込みの studio-config にある名前。
    ("bt709_oetf", "Camera Rec.709"),
];

/// `known_spaces` に出す OCIO の名前。契約は「符号化値 (1,1,1) が白」を前提にするので、
/// ACEScc や PQ のように 1.0 が白でない空間は外す。
const KNOWN_OCIO: [&str; 6] = [
    "ACEScg",
    "ACES2065-1",
    "Linear Rec.2020",
    "Linear P3-D65",
    "sRGB - Display",
    "Gamma 2.2 Rec.709 - Display",
];

struct Inner {
    config: Config,
    processors: HashMap<String, Processor>,
}

/// OCIO で色を変換する。
pub struct OcioColor {
    inner: Mutex<Inner>,
    lut_size: u32,
}

impl OcioColor {
    /// 組み込みの既定の設定で作る。
    pub fn builtin() -> Result<Self> {
        Self::open(DEFAULT_CONFIG)
    }

    /// `ocio://...` か設定ファイルのパスで作る。
    pub fn open(uri_or_path: &str) -> Result<Self> {
        let config = info_span!("color_ocio.load_config").in_scope(|| Config::open(uri_or_path))?;
        Ok(Self {
            inner: Mutex::new(Inner {
                config,
                processors: HashMap::new(),
            }),
            lut_size: DEFAULT_LUT_SIZE,
        })
    }

    /// 焼く 3D LUT の一辺を変える。
    pub fn with_lut_size(mut self, size: u32) -> Self {
        self.lut_size = size;
        self
    }

    /// 結びついている OCIO のバージョン。
    pub fn version() -> String {
        ffi::version()
    }

    /// 設定が持つ色空間の名前の全部。
    pub fn colorspace_names(&self) -> Vec<String> {
        self.lock().config.colorspace_names()
    }

    /// `from` を display と view（None は既定）で見る変換。ACES のトーンマップを含むので、
    /// 普通はテクスチャを持ち、3D LUT になる。
    pub fn display_program(
        &self,
        from: &ColorSpace,
        display: Option<&str>,
        view: Option<&str>,
    ) -> Result<TransformProgram> {
        let size = self.lut_size;
        let mut inner = self.lock();
        let p = display_processor_for(&mut inner, from, display, view)?;
        program_of(p, size)
    }

    /// `from` を display と view で見る変換を CPU で 1 画素に当てる。
    pub fn display_apply(
        &self,
        from: &ColorSpace,
        display: Option<&str>,
        view: Option<&str>,
        rgb: [f32; 3],
    ) -> Result<[f32; 3]> {
        let mut inner = self.lock();
        display_processor_for(&mut inner, from, display, view)?.apply_rgb(rgb)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("OCIO の lock")
    }
}

/// mm の名前を OCIO の名前にして、設定にあるか確かめる。
fn resolve(config: &Config, cs: &ColorSpace) -> Result<String> {
    let name = ALIASES
        .iter()
        .find(|(mm, _)| *mm == cs.0)
        .map_or(cs.0.as_str(), |(_, ocio)| ocio);
    if config.has_colorspace(name) {
        Ok(name.to_owned())
    } else {
        Err(MmError::Unsupported(format!("色空間 {}", cs.0)))
    }
}

fn processor_for<'a>(
    inner: &'a mut Inner,
    from: &ColorSpace,
    to: &ColorSpace,
) -> Result<&'a mut Processor> {
    if to.0 == DISPLAY_SPACE {
        return display_processor_for(inner, from, None, None);
    }
    let (src, dst) = (resolve(&inner.config, from)?, resolve(&inner.config, to)?);
    let key = format!("{src}|{dst}");
    if !inner.processors.contains_key(&key) {
        let p = info_span!("color_ocio.make_processor")
            .in_scope(|| inner.config.processor(&src, &dst))?;
        inner.processors.insert(key.clone(), p);
    }
    Ok(inner.processors.get_mut(&key).expect("入れたばかり"))
}

fn display_processor_for<'a>(
    inner: &'a mut Inner,
    from: &ColorSpace,
    display: Option<&str>,
    view: Option<&str>,
) -> Result<&'a mut Processor> {
    let src = resolve(&inner.config, from)?;
    let key = format!("{src}|{display:?}|{view:?}");
    if !inner.processors.contains_key(&key) {
        let p = info_span!("color_ocio.make_processor")
            .in_scope(|| inner.config.display_processor(&src, display, view))?;
        inner.processors.insert(key.clone(), p);
    }
    Ok(inner.processors.get_mut(&key).expect("入れたばかり"))
}

/// 変換から GPU の形を選ぶ。
fn program_of(p: &mut Processor, lut_size: u32) -> Result<TransformProgram> {
    if p.is_noop() {
        return Ok(TransformProgram::Identity);
    }
    let shader = info_span!("color_ocio.gen_glsl").in_scope(|| p.gpu_shader(ENTRY))?;
    if shader.num_textures == 0 && shader.num_uniforms == 0 {
        let wgsl =
            info_span!("color_ocio.naga_convert").in_scope(|| glsl_to_wgsl(&shader.text, ENTRY));
        match wgsl {
            Ok(source) => return Ok(TransformProgram::Wgsl { source }),
            Err(e) => tracing::warn!("naga が読めないので 3D LUT に焼く: {e}"),
        }
    }
    let p: &Processor = p;
    info_span!("color_ocio.bake_lut").in_scope(|| bake_lut3d(lut_size, |rgb| p.apply_rgb(rgb)))
}

impl ColorTransform for OcioColor {
    fn id(&self) -> &'static str {
        OCIO_ID
    }

    fn program(&self, from: &ColorSpace, to: &ColorSpace) -> Result<TransformProgram> {
        let size = self.lut_size;
        let mut inner = self.lock();
        let p = processor_for(&mut inner, from, to)?;
        program_of(p, size)
    }

    fn apply(&self, from: &ColorSpace, to: &ColorSpace, rgb: [f32; 3]) -> Result<[f32; 3]> {
        let mut inner = self.lock();
        processor_for(&mut inner, from, to)?.apply_rgb(rgb)
    }

    fn known_spaces(&self) -> Vec<ColorSpace> {
        let inner = self.lock();
        let aliases = ALIASES
            .iter()
            .filter(|(_, ocio)| inner.config.has_colorspace(ocio))
            .map(|(mm, _)| *mm);
        let ocio = KNOWN_OCIO
            .iter()
            .copied()
            .filter(|n| inner.config.has_colorspace(n));
        aliases.chain(ocio).map(|n| ColorSpace(n.into())).collect()
    }
}
