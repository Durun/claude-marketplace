//! 任意の変換を 3D LUT に焼く。OCIO の GPU のシェーダーが使えない（テクスチャを持つ）ときの逃げ道。

use mm_color_api::TransformProgram;
use mm_types::Result;

/// 格子の各点で `f` を呼び、`TransformProgram::Lut3d`（R が最も速く変わる）にする。
/// 格子の入力は 0..=1 を `size` 等分した点。
pub fn bake_lut3d(
    size: u32,
    mut f: impl FnMut([f32; 3]) -> Result<[f32; 3]>,
) -> Result<TransformProgram> {
    let n = size as usize;
    let step = (n - 1) as f32;
    let mut data = Vec::with_capacity(n * n * n * 3);
    for b in 0..n {
        for g in 0..n {
            for r in 0..n {
                let rgb = [r, g, b].map(|v| v as f32 / step);
                data.extend_from_slice(&f(rgb)?);
            }
        }
    }
    Ok(TransformProgram::Lut3d { size, data })
}

/// `Lut3d` を引く WGSL。バインドは `@group(1) @binding(0)` の `texture_3d<f32>`（rgba32float で
/// 一辺 `size`）。サンプラーを使わず `textureLoad` で三線形補間するので、フィルタ非対応の形式でも動く。
pub const LUT3D_APPLY_WGSL: &str = r"
@group(1) @binding(0) var mm_lut: texture_3d<f32>;

fn mm_lut_at(i: vec3<i32>) -> vec3<f32> {
    return textureLoad(mm_lut, i, 0).rgb;
}

fn mm_color_transform(c: vec3<f32>) -> vec3<f32> {
    let n = i32(textureDimensions(mm_lut).x);
    let p = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)) * f32(n - 1);
    let fl = floor(p);
    let f = p - fl;
    let i0 = vec3<i32>(fl);
    let i1 = min(i0 + vec3<i32>(1), vec3<i32>(n - 1));
    let c000 = mm_lut_at(vec3<i32>(i0.x, i0.y, i0.z));
    let c100 = mm_lut_at(vec3<i32>(i1.x, i0.y, i0.z));
    let c010 = mm_lut_at(vec3<i32>(i0.x, i1.y, i0.z));
    let c110 = mm_lut_at(vec3<i32>(i1.x, i1.y, i0.z));
    let c001 = mm_lut_at(vec3<i32>(i0.x, i0.y, i1.z));
    let c101 = mm_lut_at(vec3<i32>(i1.x, i0.y, i1.z));
    let c011 = mm_lut_at(vec3<i32>(i0.x, i1.y, i1.z));
    let c111 = mm_lut_at(vec3<i32>(i1.x, i1.y, i1.z));
    let x00 = mix(c000, c100, f.x);
    let x10 = mix(c010, c110, f.x);
    let x01 = mix(c001, c101, f.x);
    let x11 = mix(c011, c111, f.x);
    return mix(mix(x00, x10, f.y), mix(x01, x11, f.y), f.z);
}
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cube::sample_trilinear;

    #[test]
    fn 焼いたlutは元の変換に近い() {
        let f = |c: [f32; 3]| Ok(c.map(|v| v * v));
        let TransformProgram::Lut3d { size, data } = bake_lut3d(33, f).unwrap() else {
            panic!("Lut3d のはず");
        };
        let out = sample_trilinear(size, &data, [0.5, 0.25, 0.75]);
        for (a, b) in out.iter().zip([0.25, 0.0625, 0.5625]) {
            assert!((a - b).abs() < 1e-3, "{a} {b}");
        }
    }
}
