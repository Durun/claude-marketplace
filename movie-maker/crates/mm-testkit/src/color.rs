//! 色の見本と契約テスト。見本は bt709（Rec.1886）、srgb、bt709_oetf を作業空間（線形）へ変換する。

use std::sync::Arc;

use mm_color_api::{ColorTransform, TransformProgram, UNIT_WHITE_SPACES};
use mm_types::{ColorSpace, MmError, Result};

/// 往復の許容差。
pub const ROUNDTRIP_TOLERANCE: f32 = 1e-3;
/// 18% グレーの線形の値。
pub const GREY_18_LINEAR: f32 = 0.18;

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Rec.1886 の表示の式。符号を保つ。
fn bt1886_to_linear(v: f32) -> f32 {
    v.signum() * v.abs().powf(2.4)
}

fn linear_to_bt1886(v: f32) -> f32 {
    v.signum() * v.abs().powf(1.0 / 2.4)
}

/// BT.709 のカメラの OETF の逆。
fn bt709_oetf_to_linear(v: f32) -> f32 {
    if v < 0.081 {
        v / 4.5
    } else {
        ((v + 0.099) / 1.099).powf(1.0 / 0.45)
    }
}

fn linear_to_bt709_oetf(v: f32) -> f32 {
    if v < 0.018 {
        v * 4.5
    } else {
        1.099 * v.powf(0.45) - 0.099
    }
}

/// 見本の色の変換。`lin_rec709`、`srgb`（sRGB）、`bt709`（Rec.1886）、`bt709_oetf`（カメラの OETF）を知る。
#[derive(Debug, Clone, Default)]
pub struct SampleColor;

impl SampleColor {
    fn to_linear(&self, from: &ColorSpace, v: f32) -> Result<f32> {
        match from.0.as_str() {
            "lin_rec709" => Ok(v),
            "srgb" => Ok(srgb_to_linear(v)),
            "bt709" => Ok(bt1886_to_linear(v)),
            "bt709_oetf" => Ok(bt709_oetf_to_linear(v)),
            other => Err(MmError::Unsupported(format!("色空間 {other}"))),
        }
    }

    fn encode(&self, to: &ColorSpace, v: f32) -> Result<f32> {
        match to.0.as_str() {
            "lin_rec709" => Ok(v),
            "srgb" => Ok(linear_to_srgb(v)),
            "bt709" => Ok(linear_to_bt1886(v)),
            "bt709_oetf" => Ok(linear_to_bt709_oetf(v)),
            other => Err(MmError::Unsupported(format!("色空間 {other}"))),
        }
    }
}

impl ColorTransform for SampleColor {
    fn id(&self) -> &'static str {
        crate::SAMPLE_ID
    }

    fn program(&self, from: &ColorSpace, to: &ColorSpace) -> Result<TransformProgram> {
        self.to_linear(from, 0.0)?;
        self.encode(to, 0.0)?;
        if from == to {
            return Ok(TransformProgram::Identity);
        }
        Ok(TransformProgram::Wgsl {
            source: format!(
                "// sample: {} -> {}\nfn mm_color_transform(c: vec3<f32>) -> vec3<f32> {{ return c; }}\n",
                from.0, to.0
            ),
        })
    }

    fn apply(&self, from: &ColorSpace, to: &ColorSpace, rgb: [f32; 3]) -> Result<[f32; 3]> {
        let mut out = [0.0; 3];
        for (o, v) in out.iter_mut().zip(rgb) {
            *o = self.encode(to, self.to_linear(from, v)?)?;
        }
        Ok(out)
    }

    fn known_spaces(&self) -> Vec<ColorSpace> {
        vec![
            ColorSpace::working(),
            ColorSpace::srgb(),
            ColorSpace::bt709(),
            ColorSpace::bt709_oetf(),
        ]
    }
}

/// 色の契約。
pub fn contract(make: impl Fn() -> Arc<dyn ColorTransform>) {
    let ct = make();
    let working = ColorSpace::working();
    let spaces = ct.known_spaces();
    assert!(spaces.contains(&working), "作業空間を知っている");

    let same = ct.program(&working, &working).expect("作れる");
    assert_eq!(
        same,
        TransformProgram::Identity,
        "作業空間から作業空間は恒等"
    );

    for space in &spaces {
        let input = [0.1, 0.5, 0.9];
        let mid = ct.apply(space, &working, input).expect("変換");
        let back = ct.apply(&working, space, mid).expect("戻す");
        for (a, b) in input.iter().zip(back) {
            assert!(
                (a - b).abs() <= ROUNDTRIP_TOLERANCE,
                "{} の往復 {a} -> {b}",
                space.0
            );
        }
        if UNIT_WHITE_SPACES.contains(&space.0.as_str()) {
            let white = ct.apply(space, &working, [1.0; 3]).expect("白");
            for c in white {
                assert!(
                    (c - 1.0).abs() <= ROUNDTRIP_TOLERANCE,
                    "{} の白は白のまま（{c}）",
                    space.0
                );
            }
        }
        ct.program(space, &working).expect("GPU の形が作れる");
    }

    let unknown = ColorSpace("no_such_space".into());
    assert!(
        matches!(ct.program(&unknown, &working), Err(MmError::Unsupported(_))),
        "知らない色空間は Unsupported"
    );
}
