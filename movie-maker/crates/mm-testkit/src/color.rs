//! 色のモックと契約テスト。モックは bt709（Rec.1886）、srgb、bt709_oetf、bt601、bt2020 を作業色空間（線形）へ変換する。

use std::sync::Arc;

use mm_color_api::{
    ColorTransform, TransformProgram, REC2020_TO_REC709, REC709_TO_REC2020, UNIT_WHITE_SPACES,
};
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

/// モックの色の変換。`lin_rec709`、`srgb`（sRGB）、`bt709`（Rec.1886）、`bt709_oetf`（カメラの OETF）を知る。
#[derive(Debug, Clone, Default)]
pub struct SampleColor;

impl SampleColor {
    fn to_linear(&self, from: &ColorSpace, v: f32) -> Result<f32> {
        match from.0.as_str() {
            "lin_rec709" => Ok(v),
            "srgb" => Ok(srgb_to_linear(v)),
            "bt709" | "bt601" | "bt2020" => Ok(bt1886_to_linear(v)),
            "bt709_oetf" => Ok(bt709_oetf_to_linear(v)),
            other => Err(MmError::Unsupported(format!("色空間 {other}"))),
        }
    }

    fn encode(&self, to: &ColorSpace, v: f32) -> Result<f32> {
        match to.0.as_str() {
            "lin_rec709" => Ok(v),
            "srgb" => Ok(linear_to_srgb(v)),
            "bt709" | "bt601" | "bt2020" => Ok(linear_to_bt1886(v)),
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
        let mut linear = [0.0; 3];
        for (o, v) in linear.iter_mut().zip(rgb) {
            *o = self.to_linear(from, v)?;
        }
        // 原色が Rec.2020 の側だけ行列で作業色空間（Rec.709）に合わせる。
        let matrix = match (from.0 == "bt2020", to.0 == "bt2020") {
            (true, false) => Some(REC2020_TO_REC709),
            (false, true) => Some(REC709_TO_REC2020),
            _ => None,
        };
        if let Some(m) = matrix {
            linear = m.map(|row| row[0] * linear[0] + row[1] * linear[1] + row[2] * linear[2]);
        }
        let mut out = [0.0; 3];
        for (o, v) in out.iter_mut().zip(linear) {
            *o = self.encode(to, v)?;
        }
        Ok(out)
    }

    fn known_spaces(&self) -> Vec<ColorSpace> {
        vec![
            ColorSpace::working(),
            ColorSpace::srgb(),
            ColorSpace::bt709(),
            ColorSpace::bt709_oetf(),
            ColorSpace::bt601(),
            ColorSpace::bt2020(),
        ]
    }
}

/// 色の契約。
pub fn contract(make: impl Fn() -> Arc<dyn ColorTransform>) {
    let ct = make();
    let working = ColorSpace::working();
    let spaces = ct.known_spaces();
    assert!(spaces.contains(&working), "作業色空間を知っている");

    let same = ct.program(&working, &working).expect("作れる");
    assert_eq!(
        same,
        TransformProgram::Identity,
        "作業色空間から作業色空間は恒等"
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

    named_spaces_contract(ct.as_ref());

    let unknown = ColorSpace("no_such_space".into());
    assert!(
        matches!(ct.program(&unknown, &working), Err(MmError::Unsupported(_))),
        "知らない色空間は Unsupported"
    );
}

/// 名前の表（RFC 0005）の全部を引けること、`bt601` は `bt709` と同じ変換、`bt2020` は原色を
/// Rec.709 に変えることを確かめる。デコーダーが SD や UHD の素材をこの名前で名乗るため。
fn named_spaces_contract(ct: &dyn ColorTransform) {
    let working = ColorSpace::working();
    for name in UNIT_WHITE_SPACES {
        let cs = ColorSpace((*name).into());
        ct.program(&cs, &working)
            .unwrap_or_else(|e| panic!("{name} の program を引ける: {e}"));
        ct.apply(&cs, &working, [0.2, 0.4, 0.6])
            .unwrap_or_else(|e| panic!("{name} の apply を引ける: {e}"));
    }

    let probe = [0.1, 0.5, 0.9];
    let sd = ct
        .apply(&ColorSpace::bt601(), &working, probe)
        .expect("bt601");
    let hd = ct
        .apply(&ColorSpace::bt709(), &working, probe)
        .expect("bt709");
    for (a, b) in sd.iter().zip(hd) {
        assert!(
            (a - b).abs() <= ROUNDTRIP_TOLERANCE,
            "bt601 は bt709 と同じ変換"
        );
    }

    // 符号化値 (1,0,0) は線形でも (1,0,0)。作業色空間では行列の第 1 列になる。
    let red = ct
        .apply(&ColorSpace::bt2020(), &working, [1.0, 0.0, 0.0])
        .expect("bt2020");
    for (i, got) in red.iter().enumerate() {
        let want = REC2020_TO_REC709[i][0];
        assert!(
            (got - want).abs() <= ROUNDTRIP_TOLERANCE,
            "bt2020 の赤は Rec.709 の原色に直る ch{i}: {got} vs {want}"
        );
    }
}
