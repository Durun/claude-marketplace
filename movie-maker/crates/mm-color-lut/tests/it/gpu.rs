//! WGSL を GPU で実行して CPU の値と突き合わせる。`--features gpu-eval` のときだけ動き、
//! アダプターが無ければスキップする。

use mm_color_api::{ColorTransform, TransformProgram};
use mm_color_lut::gpu_eval::GpuEval;
use mm_color_lut::{bake_lut3d, sample_trilinear, Cdl, LutColor, LUT3D_APPLY_WGSL};

/// テスト用の色。黒、18% グレー、白、原色、彩度の高い色。
const SAMPLES: [[f32; 3]; 8] = [
    [0.0, 0.0, 0.0],
    [0.18, 0.18, 0.18],
    [1.0, 1.0, 1.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.8, 0.4, 0.1],
    [0.02, 0.5, 0.97],
];

fn max_diff(a: &[[f32; 3]], b: &[[f32; 3]]) -> f32 {
    a.iter()
        .zip(b)
        .flat_map(|(x, y)| x.iter().zip(y).map(|(p, q)| (p - q).abs()))
        .fold(0.0, f32::max)
}

#[test]
fn 伝達関数のwgslがcpuと一致する() {
    let Some(gpu) = GpuEval::new() else {
        eprintln!("アダプターが無いのでスキップする");
        return;
    };
    let ct = LutColor;
    let spaces = ct.known_spaces();
    for from in &spaces {
        for to in &spaces {
            let TransformProgram::Wgsl { source } = ct.program(from, to).unwrap() else {
                continue;
            };
            let got = gpu.run(&source, &SAMPLES, None).expect("GPU で実行");
            let want: Vec<_> = SAMPLES
                .iter()
                .map(|&c| ct.apply(from, to, c).unwrap())
                .collect();
            let d = max_diff(&got, &want);
            assert!(d < 1e-4, "{} -> {}: 差 {d}", from.0, to.0);
        }
    }
}

#[test]
fn cdlのwgslがcpuと一致する() {
    let Some(gpu) = GpuEval::new() else {
        return;
    };
    let cdl = Cdl {
        slope: [1.2, 0.9, 1.0],
        offset: [0.01, -0.02, 0.0],
        power: [1.1, 0.95, 1.0],
        saturation: 0.8,
    };
    let got = gpu.run(&cdl.to_wgsl(), &SAMPLES, None).expect("GPU で実行");
    let want: Vec<_> = SAMPLES.iter().map(|&c| cdl.apply(c)).collect();
    assert!(max_diff(&got, &want) < 1e-4);
}

#[test]
fn lutのwgslがcpuの補間と一致する() {
    let Some(gpu) = GpuEval::new() else {
        return;
    };
    let TransformProgram::Lut3d { size, data } =
        bake_lut3d(17, |c| Ok(c.map(|v| v.powf(2.2)))).unwrap()
    else {
        panic!("Lut3d のはず");
    };
    let got = gpu
        .run(LUT3D_APPLY_WGSL, &SAMPLES, Some((size, &data)))
        .expect("GPU で実行");
    let want: Vec<_> = SAMPLES
        .iter()
        .map(|&c| sample_trilinear(size, &data, c))
        .collect();
    assert!(max_diff(&got, &want) < 1e-4);
}
