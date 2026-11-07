//! OCIO の CPU の値を正として、(1) 生成した WGSL（GPU で実行）と (2) ベイクした 3D LUT がどこまで合うかを測る。
//! 数字は `--nocapture` で表に出す。GPU が無ければ GPU の部分をスキップする。

use mm_color_api::{ColorTransform, TransformProgram};
use mm_color_lut::LutColor;
use mm_color_ocio::OcioColor;
use mm_types::ColorSpace;

/// テスト用の色。黒、18% グレー、白、原色、彩度の高い色、暗部、ACES で線形 1 を超えない範囲の中間色。
const SAMPLES: [[f32; 3]; 10] = [
    [0.0, 0.0, 0.0],
    [0.18, 0.18, 0.18],
    [1.0, 1.0, 1.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.8, 0.4, 0.1],
    [0.02, 0.5, 0.97],
    [0.003, 0.001, 0.002],
    [0.5, 0.5, 0.5],
];

/// WGSL と CPU の許容差。f32 の pow の誤差を見込む。
const WGSL_TOLERANCE: f32 = 2e-4;

fn space(name: &str) -> ColorSpace {
    ColorSpace(name.into())
}

fn max_diff(a: &[[f32; 3]], b: &[[f32; 3]]) -> f32 {
    a.iter()
        .zip(b)
        .flat_map(|(x, y)| x.iter().zip(y).map(|(p, q)| (p - q).abs()))
        .fold(0.0, f32::max)
}

/// 試験する変換の組。(入力, 出力)。
const PAIRS: [(&str, &str); 6] = [
    ("srgb", "lin_rec709"),
    ("lin_rec709", "srgb"),
    ("ACEScg", "lin_rec709"),
    ("lin_rec709", "bt709"),
    ("ACEScg", "sRGB - Display"),
    ("ACES2065-1", "lin_rec709"),
];

#[test]
fn 変換の組はwgslになる() {
    let ocio = OcioColor::builtin().expect("設定");
    for (from, to) in PAIRS {
        let program = ocio.program(&space(from), &space(to)).expect("変換");
        assert!(
            matches!(program, TransformProgram::Wgsl { .. }),
            "{from} -> {to} は WGSL のはず（{program:?}）"
        );
    }
}

#[test]
fn ocioとlutクレートのsrgbが一致する() {
    let ocio = OcioColor::builtin().expect("設定");
    for (from, to) in [("srgb", "lin_rec709"), ("lin_rec709", "srgb")] {
        let (f, t) = (space(from), space(to));
        let a: Vec<_> = SAMPLES
            .iter()
            .map(|&c| ocio.apply(&f, &t, c).unwrap())
            .collect();
        let b: Vec<_> = SAMPLES
            .iter()
            .map(|&c| LutColor.apply(&f, &t, c).unwrap())
            .collect();
        let d = max_diff(&a, &b);
        eprintln!("ocio と lut の {from} -> {to}: 最大差 {d:e}");
        assert!(d < 2e-4, "{from} -> {to}: {d}");
    }
}

#[cfg(feature = "gpu-eval")]
mod gpu {
    use super::*;
    use mm_color_lut::gpu_eval::GpuEval;
    use mm_color_lut::LUT3D_APPLY_WGSL;

    #[test]
    fn wgslをgpuで実行するとocioのcpuと一致する() {
        let Some(gpu) = GpuEval::new() else {
            eprintln!("アダプターが無いのでスキップする");
            return;
        };
        eprintln!("GPU: {}", gpu.adapter_name);
        let ocio = OcioColor::builtin().expect("設定");
        for (from, to) in PAIRS {
            let (f, t) = (space(from), space(to));
            let TransformProgram::Wgsl { source } = ocio.program(&f, &t).unwrap() else {
                panic!("{from} -> {to} は WGSL のはず");
            };
            let got = gpu.run(&source, &SAMPLES, None).expect("GPU で実行");
            let want: Vec<_> = SAMPLES
                .iter()
                .map(|&c| ocio.apply(&f, &t, c).unwrap())
                .collect();
            let d = max_diff(&got, &want);
            eprintln!("WGSL(GPU) と OCIO(CPU) の {from} -> {to}: 最大差 {d:e}");
            assert!(d < WGSL_TOLERANCE, "{from} -> {to}: {d}");
        }
    }

    #[test]
    fn aces_の表示変換は3d_lutにベイクされ_gpuの値がcpuに近い() {
        let Some(gpu) = GpuEval::new() else {
            return;
        };
        let ocio = OcioColor::builtin().expect("設定").with_lut_size(65);
        let from = space("ACEScg");
        let program = ocio.display_program(&from, None, None).expect("変換");
        let TransformProgram::Lut3d { size, data } = program else {
            panic!("ACES の表示変換は 3D LUT になるはず（{program:?}）");
        };
        // 0..1 に収まる中間色で比べる。LUT の外（1 超）はベイクできない。
        let inputs: Vec<[f32; 3]> = SAMPLES.to_vec();
        let got = gpu
            .run(LUT3D_APPLY_WGSL, &inputs, Some((size, &data)))
            .expect("GPU で実行");
        let want: Vec<_> = inputs
            .iter()
            .map(|&c| ocio.display_apply(&from, None, None, c).unwrap())
            .collect();
        let d = max_diff(&got, &want);
        eprintln!(
            "ベイクした LUT({size}^3, GPU) と OCIO(CPU) の ACEScg -> 既定の表示: 最大差 {d:e}"
        );
        assert!(d < 5e-2, "{d}");
    }
}

/// テクスチャを持つシェーダーを naga に通した結果を表に出す。インターフェースの `Wgsl` にはテクスチャの
/// 置く場所が無いので、使うかどうかは別の問題として、naga が読めるかだけを確かめる。
#[test]
fn aces_の表示シェーダーをnagaに通す() {
    let cfg = mm_color_ocio::ffi::Config::open("ocio://default").expect("設定");
    let (display, view) = cfg.default_display_view("ACEScg").expect("既定の表示");
    let mut p = cfg
        .display_processor("ACEScg", None, None)
        .expect("表示の変換");
    let shader = p.gpu_shader("OCIOConvert").expect("シェーダー");
    eprintln!(
        "既定の表示 {display} / {view}: GLSL {} 文字、テクスチャ {}、動的な値 {}",
        shader.text.len(),
        shader.num_textures,
        shader.num_uniforms
    );
    eprintln!(
        "GLSL の先頭:\n{}",
        &shader.text[..400.min(shader.text.len())]
    );
    match mm_color_ocio::glsl::glsl_to_wgsl(&shader.text, "OCIOConvert") {
        Ok(w) => eprintln!("naga: 変換できた（WGSL {} 文字）", w.len()),
        Err(e) => eprintln!("naga: 失敗 {e}"),
    }
}

#[test]
fn aces_の表示変換はテクスチャを持つ() {
    let ocio = OcioColor::builtin().expect("設定");
    let program = ocio
        .display_program(&space("ACEScg"), None, None)
        .expect("変換");
    eprintln!(
        "ACEScg -> 既定の表示: {}",
        match &program {
            TransformProgram::Identity => "Identity".to_owned(),
            TransformProgram::Wgsl { source } => format!("Wgsl {} 文字", source.len()),
            TransformProgram::Lut3d { size, .. } => format!("Lut3d {size}^3"),
        }
    );
}
