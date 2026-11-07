//! OCIO の無い環境では feature `native` を付けず、このファイルは空になる（README を見る）。

use std::sync::Arc;

use mm_color_api::{ColorTransform, TransformProgram};
use mm_color_ocio::{OcioColor, DISPLAY_SPACE};
use mm_types::ColorSpace;

#[test]
fn 色の契約を満たす() {
    mm_testkit::color::contract(|| Arc::new(OcioColor::builtin().expect("組み込みの設定")));
}

#[test]
fn 名前の表の空間を全部知っている() {
    let known = OcioColor::builtin().expect("設定").known_spaces();
    for name in mm_color_api::UNIT_WHITE_SPACES {
        assert!(known.contains(&ColorSpace((*name).into())), "{name} を知る");
    }
}

#[test]
fn bt2020_の行列は_ocio_の_linear_rec2020_と一致する() {
    let ocio = OcioColor::builtin().expect("設定");
    let (from, to) = (ColorSpace("Linear Rec.2020".into()), ColorSpace::working());
    for (i, unit) in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        .iter()
        .enumerate()
    {
        let got = ocio.apply(&from, &to, *unit).expect("OCIO");
        for (row, g) in got.iter().enumerate() {
            let want = mm_color_api::REC2020_TO_REC709[row][i];
            assert!((g - want).abs() < 1e-3, "[{row}][{i}] {g} vs {want}");
        }
    }
}

#[test]
fn aces_の表示変換は_3d_lut_で返り格子の値がapplyと一致する() {
    let ocio = OcioColor::builtin().expect("設定").with_lut_size(9);
    let (from, to) = (
        ColorSpace("ACEScg".into()),
        ColorSpace(DISPLAY_SPACE.into()),
    );
    let TransformProgram::Lut3d { size, data } = ocio.program(&from, &to).expect("変換") else {
        panic!("ACES の表示変換は 3D LUT になるはず");
    };
    let n = size as usize;
    assert_eq!(data.len(), n * n * n * 3);
    assert!(data.iter().all(|v| v.is_finite()));
    // R が最も速く変わる順序の、格子の 1 点（r=2, g=5, b=7）。
    let step = (n - 1) as f32;
    let rgb = [2.0 / step, 5.0 / step, 7.0 / step];
    let want = ocio.apply(&from, &to, rgb).expect("CPU");
    let at = ((7 * n + 5) * n + 2) * 3;
    for (i, w) in want.iter().enumerate() {
        assert!(
            (data[at + i] - w).abs() < 1e-6,
            "ch{i}: {} vs {w}",
            data[at + i]
        );
    }
    assert!(
        !ocio.known_spaces().contains(&to),
        "逆変換が無いので known_spaces に出さない"
    );
}
