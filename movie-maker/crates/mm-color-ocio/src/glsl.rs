//! OCIO が出す GLSL の関数を naga で WGSL にし、`mm_color_transform` のエントリーポイントを足す。
//! OCIO は関数だけを出す（エントリーポイントの `main` が無い）ので、空の `main` を足して読ませ、
//! 変換後にエントリーポイントを破棄して関数だけを書き出す。

use mm_types::{MmError, Result};
use naga::back::wgsl::WriterFlags;
use naga::front::glsl::{Frontend, Options};
use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga::ShaderStage;

/// OCIO のシェーダー（`function_name` をエントリーポイントに持つ GLSL の断片）を WGSL にする。
/// 戻り値は `fn mm_color_transform(c: vec3<f32>) -> vec3<f32>` を含む。
pub fn glsl_to_wgsl(ocio_glsl: &str, function_name: &str) -> Result<String> {
    // main は naga がエントリーポイントとして要求するだけで、内容は使わない。
    let source =
        format!("#version 450\n{ocio_glsl}\nvoid main() {{ {function_name}(vec4(0.0)); }}\n");
    let mut module = Frontend::default()
        .parse(&Options::from(ShaderStage::Fragment), &source)
        .map_err(|e| MmError::Unsupported(format!("GLSL を naga が読めない: {e:?}")))?;
    module.entry_points.clear();

    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .map_err(|e| MmError::Unsupported(format!("GLSL の変換結果が検証に失敗した: {e:?}")))?;
    let mut wgsl = naga::back::wgsl::write_string(&module, &info, WriterFlags::empty())
        .map_err(|e| MmError::Unsupported(format!("WGSL に書けない: {e:?}")))?;

    wgsl.push_str(&format!(
        "\nfn mm_color_transform(c: vec3<f32>) -> vec3<f32> {{\n    \
         return {function_name}(vec4<f32>(c, 1.0)).rgb;\n}}\n"
    ));
    Ok(wgsl)
}

/// 変換した WGSL を naga で読み直して検証する。
pub fn validate_wgsl(source: &str) -> Result<()> {
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|e| MmError::Corrupt(format!("WGSL が読めない: {}", e.emit_to_string(source))))?;
    Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .map_err(|e| MmError::Corrupt(format!("WGSL の検証に失敗した: {e:?}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// OCIO 2.6 が `sRGB Encoded Rec.709 (sRGB)` → `Linear Rec.709 (sRGB)` で出した GLSL（GLSL_VK_4_6）。
    const OCIO_SRGB_TO_LINEAR: &str = r"
vec4 OCIOConvert(vec4 inPixel)
{
  vec4 outColor = inPixel;
  {
    vec4 breakPnt = vec4(0.0392857157, 0.0392857157, 0.0392857157, 1.);
    vec4 slope = vec4(0.077380158, 0.077380158, 0.077380158, 1.);
    vec4 scale = vec4(0.947867274, 0.947867274, 0.947867274, 0.999998987);
    vec4 offset = vec4(0.0521326996, 0.0521326996, 0.0521326996, 9.99998974e-07);
    vec4 gamma = vec4(2.4000001, 2.4000001, 2.4000001, 1.00000095);
    vec4 isAboveBreak = vec4(greaterThan( outColor, breakPnt));
    vec4 linSeg = outColor * slope;
    vec4 powSeg = pow( max( vec4(0., 0., 0., 0.), scale * outColor + offset), gamma);
    vec4 res = isAboveBreak * powSeg + ( vec4(1., 1., 1., 1.) - isAboveBreak ) * linSeg;
    outColor.rgb = vec3(res.x, res.y, res.z);
    outColor.a = res.w;
  }
  return outColor;
}
";

    #[test]
    fn ocioのglslをwgslにして検証を通す() {
        let wgsl = glsl_to_wgsl(OCIO_SRGB_TO_LINEAR, "OCIOConvert").expect("変換");
        assert!(wgsl.contains("fn mm_color_transform"));
        validate_wgsl(&wgsl).unwrap_or_else(|e| panic!("{e}\n{wgsl}"));
    }

    /// naga の GLSL 入力は結合サンプラー（`sampler3D`）を読めない。OCIO は結合の形で出すので、
    /// テクスチャを持つシェーダー（ACES 2.0 の表示変換など）はこの変換器では通らない。
    #[test]
    fn 結合サンプラーは読めない() {
        for kind in ["sampler1D", "sampler2D", "sampler3D"] {
            let glsl = format!(
                "layout(set=0, binding = 1) uniform {kind} t;
vec4 OCIOConvert(vec4 p) {{ return texture(t, p.xyz); }}"
            );
            assert!(
                matches!(
                    glsl_to_wgsl(&glsl, "OCIOConvert"),
                    Err(MmError::Unsupported(_))
                ),
                "{kind}"
            );
        }
    }

    /// テクスチャとサンプラーに分ければ読める。結合サンプラーをこの形に書き換えれば
    /// テクスチャ付きも通る見込みだが、`TransformProgram::Wgsl` にテクスチャを置く場所が無いので
    /// 試作では 3D LUT にベイクする道を取る。
    #[test]
    fn 分けたテクスチャとサンプラーは読める() {
        for (kind, coord) in [("1D", "p.x"), ("2D", "p.xy"), ("3D", "p.xyz")] {
            let glsl = format!(
                "layout(set=0, binding = 1) uniform texture{kind} t_tex;
                 layout(set=0, binding = 2) uniform sampler t_smp;
                 vec4 OCIOConvert(vec4 p) {{ return texture(sampler{kind}(t_tex, t_smp), {coord}); }}"
            );
            let wgsl = glsl_to_wgsl(&glsl, "OCIOConvert").unwrap_or_else(|e| panic!("{kind}: {e}"));
            validate_wgsl(&wgsl).unwrap_or_else(|e| {
                panic!(
                    "{kind}: {e}
{wgsl}"
                )
            });
        }
    }

    #[test]
    fn 読めないglslはunsupportedで返す() {
        let err = glsl_to_wgsl("vec4 OCIOConvert(vec4 p) { return nope; }", "OCIOConvert")
            .expect_err("失敗するはず");
        assert!(matches!(err, MmError::Unsupported(_)));
    }
}
