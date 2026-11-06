use std::sync::Arc;

use mm_color_api::{ColorTransform, TransformProgram};
use mm_color_lut::{Cdl, LutColor, LUT3D_APPLY_WGSL};
use mm_types::ColorSpace;

#[test]
fn 色の契約を満たす() {
    mm_testkit::color::contract(|| Arc::new(LutColor));
}

/// WGSL を naga で読んで検証する。GPU が無くても回る。
fn validate(source: &str) {
    let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| {
        panic!("{}\n{source}", e.emit_to_string(source));
    });
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("{e:?}\n{source}"));
}

#[test]
fn 全部の組のwgslが検証を通る() {
    let ct = LutColor;
    let spaces = ct.known_spaces();
    for from in &spaces {
        for to in &spaces {
            if let TransformProgram::Wgsl { source } = ct.program(from, to).unwrap() {
                validate(&source);
            }
        }
    }
}

#[test]
fn cdlとlutのwgslが検証を通る() {
    validate(&Cdl::default().to_wgsl());
    validate(LUT3D_APPLY_WGSL);
}

#[test]
fn ベイクしたlutは伝達関数に近い() {
    let ct = LutColor;
    let (srgb, working) = (ColorSpace::srgb(), ColorSpace::working());
    let TransformProgram::Lut3d { size, data } = ct.bake(&srgb, &working, 65).unwrap() else {
        panic!("Lut3d のはず");
    };
    let rgb = [0.25, 0.5, 0.75];
    let exact = ct.apply(&srgb, &working, rgb).unwrap();
    let baked = mm_color_lut::sample_trilinear(size, &data, rgb);
    for (a, b) in exact.iter().zip(baked) {
        assert!((a - b).abs() < 2e-3, "{a} {b}");
    }
}
