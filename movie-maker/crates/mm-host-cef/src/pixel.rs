//! CEF の画素（BGRA8、premultiplied、sRGB の符号化）を、`SharedFrame::Cpu` の形
//! （RGBA f32、premultiplied、線形）に直す。

/// 8 bit の最大値。
const MAX8: f32 = 255.0;

/// sRGB の符号化を線形に戻す（0..=1）。
fn decode_srgb(s: f32) -> f32 {
    if s <= 0.040_45 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// 8 bit の sRGB から線形への表。不透明な画素はこれだけで済む。
fn table() -> [f32; 256] {
    let mut t = [0.0; 256];
    for (i, v) in t.iter_mut().enumerate() {
        *v = decode_srgb(i as f32 / MAX8);
    }
    t
}

/// BGRA8 の行（行の間に詰め物が無い）を、線形の premultiplied の RGBA f32 にする。
///
/// CEF は sRGB の符号化のまま premultiplied で渡すので、半透明の画素は一度 alpha で割ってから線形にし、
/// alpha を掛け直す（符号化の空間で掛けた値をそのまま線形にすると暗くなる）。
pub fn bgra8_to_linear_rgba(bgra: &[u8]) -> Vec<f32> {
    let lut = table();
    let mut out = Vec::with_capacity(bgra.len());
    for px in bgra.chunks_exact(4) {
        let (b, g, r, a) = (px[0], px[1], px[2], px[3]);
        if a == u8::MAX {
            out.extend([lut[r as usize], lut[g as usize], lut[b as usize], 1.0]);
            continue;
        }
        if a == 0 {
            out.extend([0.0; 4]);
            continue;
        }
        let alpha = f32::from(a) / MAX8;
        let lin = |c: u8| decode_srgb((f32::from(c) / f32::from(a)).min(1.0)) * alpha;
        out.extend([lin(r), lin(g), lin(b), alpha]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 不透明な画素は並びを入れ替えて線形にする() {
        let out = bgra8_to_linear_rgba(&[0, 128, 255, 255]);
        assert_eq!(out[0], 1.0, "R は元の 3 番目");
        assert!(
            (out[1] - 0.2158).abs() < 1e-3,
            "sRGB 128 は線形で約 0.216: {}",
            out[1]
        );
        assert_eq!(out[2], 0.0);
        assert_eq!(out[3], 1.0);
    }

    #[test]
    fn 半透明の画素は割ってから線形にし掛け直す() {
        // premultiplied の白の 50%（sRGB の符号化で 128, alpha 128）は、線形でも白の 50%。
        let out = bgra8_to_linear_rgba(&[128, 128, 128, 128]);
        let alpha = 128.0 / 255.0;
        for c in &out[..3] {
            assert!((c - alpha).abs() < 1e-6, "{c}");
        }
        assert_eq!(out[3], alpha);
    }

    #[test]
    fn 透明な画素は全部が_0() {
        assert_eq!(bgra8_to_linear_rgba(&[9, 9, 9, 0]), vec![0.0; 4]);
    }
}
