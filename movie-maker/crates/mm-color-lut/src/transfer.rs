//! 伝達関数（sRGB、Rec.1886、BT.709 の OETF）の CPU 実装と、同じ式の WGSL。
//! 作業空間は線形の Rec.709 なので、これらで入出力の 4 つの空間が閉じる。

/// sRGB の符号化値から線形へ。
pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// 線形から sRGB の符号化値へ。
pub fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Rec.1886 の表示の式。符号化値から線形へ（べき 2.4、負は符号を保つ）。
pub fn bt1886_to_linear(v: f32) -> f32 {
    v.signum() * v.abs().powf(2.4)
}

/// Rec.1886 の表示の式の逆。線形から符号化値へ。
pub fn linear_to_bt1886(v: f32) -> f32 {
    v.signum() * v.abs().powf(1.0 / 2.4)
}

/// BT.709 のカメラの OETF の逆（`bt709_oetf`）。符号化値から線形へ。
pub fn bt709_to_linear(v: f32) -> f32 {
    if v < 0.081 {
        v / 4.5
    } else {
        ((v + 0.099) / 1.099).powf(1.0 / 0.45)
    }
}

/// BT.709 の OETF。線形から符号化値へ。
pub fn linear_to_bt709(v: f32) -> f32 {
    if v < 0.018 {
        v * 4.5
    } else {
        1.099 * v.powf(0.45) - 0.099
    }
}

/// WGSL の伝達関数。`mm_color_transform` を作る側がここから関数を選んで呼ぶ。
/// 負の値は符号を保って折り返す（CPU 側は負を通さない前提なので 0 以上では同じ値になる）。
pub const TRANSFER_WGSL: &str = r"
fn mm_srgb_to_linear(v: f32) -> f32 {
    let s = sign(v);
    let a = abs(v);
    if a <= 0.04045 { return s * a / 12.92; }
    return s * pow((a + 0.055) / 1.055, 2.4);
}

fn mm_linear_to_srgb(v: f32) -> f32 {
    let s = sign(v);
    let a = abs(v);
    if a <= 0.0031308 { return s * a * 12.92; }
    return s * (1.055 * pow(a, 1.0 / 2.4) - 0.055);
}

fn mm_bt1886_to_linear(v: f32) -> f32 {
    return sign(v) * pow(abs(v), 2.4);
}

fn mm_linear_to_bt1886(v: f32) -> f32 {
    return sign(v) * pow(abs(v), 1.0 / 2.4);
}

fn mm_bt709_to_linear(v: f32) -> f32 {
    let s = sign(v);
    let a = abs(v);
    if a < 0.081 { return s * a / 4.5; }
    return s * pow((a + 0.099) / 1.099, 1.0 / 0.45);
}

fn mm_linear_to_bt709(v: f32) -> f32 {
    let s = sign(v);
    let a = abs(v);
    if a < 0.018 { return s * a * 4.5; }
    return s * (1.099 * pow(a, 0.45) - 0.099);
}
";
