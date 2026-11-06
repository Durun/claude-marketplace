//! 伝達関数（sRGB、Rec.1886、BT.709 の OETF）の CPU 実装と、同じ式の WGSL。
//! 広色域（`bt2020`）の色は Rec.709 の線形で負になるので、どの関数も負の値を原点対称に折り返す
//! （WGSL と同じ。`sign(v) * f(abs(v))`）。

/// sRGB の符号化値から線形へ。
pub fn srgb_to_linear(v: f32) -> f32 {
    let a = v.abs();
    let l = if a <= 0.040_45 {
        a / 12.92
    } else {
        ((a + 0.055) / 1.055).powf(2.4)
    };
    v.signum() * l
}

/// 線形から sRGB の符号化値へ。
pub fn linear_to_srgb(v: f32) -> f32 {
    let a = v.abs();
    let e = if a <= 0.003_130_8 {
        a * 12.92
    } else {
        1.055 * a.powf(1.0 / 2.4) - 0.055
    };
    v.signum() * e
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
    let a = v.abs();
    let l = if a < 0.081 {
        a / 4.5
    } else {
        ((a + 0.099) / 1.099).powf(1.0 / 0.45)
    };
    v.signum() * l
}

/// BT.709 の OETF。線形から符号化値へ。
pub fn linear_to_bt709(v: f32) -> f32 {
    let a = v.abs();
    let e = if a < 0.018 {
        a * 4.5
    } else {
        1.099 * a.powf(0.45) - 0.099
    };
    v.signum() * e
}

/// WGSL の伝達関数。`mm_color_transform` を作る側がここから関数を選んで呼ぶ。CPU 側と同じ式で、
/// 負の値は符号を保って折り返す。
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
