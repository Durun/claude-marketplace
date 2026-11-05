//! YUV 4:2:0（NV12 と P010）から、線形の RGB への変換の係数と WGSL。
//!
//! CPU の経路（`mm-decode-ffmpeg`）と同じく、符号化値に Rec.1886 の式（べき 2.4）を当てて線形にする。
//! 10 bit は符号語のまま f32 で扱い、8 bit に落とさない。

use ff::color::{Range, Space};
use ffmpeg_next as ff;
use mm_types::ColorSpace;

/// 表示の式のべき（`mm-color-lut` の `bt1886_to_linear` と同じ）。
const GAMMA: f32 = 2.4;

/// YUV から RGB への行列。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matrix {
    Bt601,
    Bt709,
    Bt2020,
}

impl Matrix {
    /// フレームの宣言から。未指定は画の高さで決める（HD 以上は 709）。
    pub fn from_frame(space: Space, height: u32) -> Self {
        match space {
            Space::BT709 => Self::Bt709,
            Space::BT2020NCL | Space::BT2020CL => Self::Bt2020,
            Space::SMPTE170M | Space::BT470BG | Space::FCC | Space::SMPTE240M => Self::Bt601,
            _ if height >= 720 => Self::Bt709,
            _ => Self::Bt601,
        }
    }

    /// 文書に出す色空間の名前。
    pub fn color_space(self) -> ColorSpace {
        match self {
            Self::Bt601 => ColorSpace::bt601(),
            Self::Bt709 => ColorSpace::bt709(),
            Self::Bt2020 => ColorSpace::bt2020(),
        }
    }

    /// 輝度の係数 (Kr, Kb)。
    fn coefficients(self) -> (f32, f32) {
        match self {
            Self::Bt601 => (0.299, 0.114),
            Self::Bt709 => (0.2126, 0.0722),
            Self::Bt2020 => (0.2627, 0.0593),
        }
    }
}

/// フレームのレンジの宣言。未指定は tv レンジ。
pub fn is_full_range(range: Range) -> bool {
    matches!(range, Range::JPEG)
}

/// 変換のパラメーター。符号語の単位（8 bit なら 0..255、10 bit なら 0..1023）で持つ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct YuvParams {
    /// テクスチャの値（0..1）→ 符号語。バッファを直接読むときは 1。
    pub code_scale: f32,
    pub y_off: f32,
    pub y_range: f32,
    pub c_mid: f32,
    pub c_range: f32,
    pub rv: f32,
    pub gu: f32,
    pub gv: f32,
    pub bu: f32,
}

/// 一様バッファに置くときのバイト数（`vec4` 3 つ分）。
pub const PARAMS_BYTES: usize = 48;

impl YuvParams {
    /// 行列、レンジ、ビット深度から。`code_scale` は呼び出し側が入れる。
    pub fn new(matrix: Matrix, full_range: bool, bits: u32, code_scale: f32) -> Self {
        let (kr, kb) = matrix.coefficients();
        let kg = 1.0 - kr - kb;
        let shift = (bits - 8) as f32;
        let unit = 2f32.powf(shift);
        let max_code = (1u32 << bits) as f32 - 1.0;
        let (y_off, y_range, c_range) = if full_range {
            (0.0, max_code, max_code)
        } else {
            (16.0 * unit, 219.0 * unit, 224.0 * unit)
        };
        let rv = 2.0 * (1.0 - kr);
        let bu = 2.0 * (1.0 - kb);
        Self {
            code_scale,
            y_off,
            y_range,
            c_mid: 128.0 * unit,
            c_range,
            rv,
            gu: bu * kb / kg,
            gv: rv * kr / kg,
            bu,
        }
    }

    /// 16 bit のテクスチャ（P010 は上位 10 bit に詰める）の値 → 符号語の倍率。
    pub fn texture_code_scale(bits: u32) -> f32 {
        if bits == 8 {
            255.0
        } else {
            65535.0 / (1u32 << (16 - bits)) as f32
        }
    }

    /// 一様バッファの中身。
    pub fn to_bytes(self) -> [u8; PARAMS_BYTES] {
        let fields = [
            self.code_scale,
            self.y_off,
            self.y_range,
            self.c_mid,
            self.c_range,
            self.rv,
            self.gu,
            self.gv,
            self.bu,
            0.0,
            0.0,
            0.0,
        ];
        let mut out = [0u8; PARAMS_BYTES];
        for (chunk, v) in out.chunks_exact_mut(4).zip(fields) {
            chunk.copy_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// 符号語から線形の RGB（CPU で同じ式を当てる確かめ用）。
    pub fn to_linear(self, y: f32, u: f32, v: f32) -> [f32; 3] {
        let luma = (y - self.y_off) / self.y_range;
        let cb = (u - self.c_mid) / self.c_range;
        let cr = (v - self.c_mid) / self.c_range;
        let rgb = [
            luma + self.rv * cr,
            luma - self.gu * cb - self.gv * cr,
            luma + self.bu * cb,
        ];
        rgb.map(|c| c.clamp(0.0, 1.0).powf(GAMMA))
    }
}

/// WGSL の共通部。呼ぶ側が `var<uniform> params: Params` を宣言する。
pub const YUV_WGSL: &str = r"
struct Params {
    code_scale: f32,
    y_off: f32,
    y_range: f32,
    c_mid: f32,
    c_range: f32,
    rv: f32,
    gu: f32,
    gv: f32,
    bu: f32,
    pad0: f32,
    pad1: f32,
    pad2: f32,
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

fn yuv_to_linear(y: f32, u: f32, v: f32) -> vec4<f32> {
    let luma = (y - params.y_off) / params.y_range;
    let cb = (u - params.c_mid) / params.c_range;
    let cr = (v - params.c_mid) / params.c_range;
    let rgb = clamp(
        vec3<f32>(luma + params.rv * cr, luma - params.gu * cb - params.gv * cr, luma + params.bu * cb),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );
    return vec4<f32>(pow(rgb, vec3<f32>(2.4)), 1.0);
}
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 限定レンジの白と黒が_0_と_1_になる() {
        let p = YuvParams::new(Matrix::Bt709, false, 8, 255.0);
        let white = p.to_linear(235.0, 128.0, 128.0);
        let black = p.to_linear(16.0, 128.0, 128.0);
        assert!(white.iter().all(|c| (c - 1.0).abs() < 1e-5), "{white:?}");
        assert!(black.iter().all(|c| c.abs() < 1e-5), "{black:?}");
    }

    #[test]
    fn 十ビットは八ビットと同じ明るさを細かい段で表す() {
        let p8 = YuvParams::new(Matrix::Bt709, false, 8, 255.0);
        let p10 = YuvParams::new(Matrix::Bt709, false, 10, YuvParams::texture_code_scale(10));
        let a = p8.to_linear(126.0, 128.0, 128.0)[0];
        let b = p10.to_linear(504.0, 512.0, 512.0)[0];
        assert!((a - b).abs() < 1e-5, "{a} {b}");
        let step8 = p8.to_linear(127.0, 128.0, 128.0)[0] - a;
        let step10 = p10.to_linear(505.0, 512.0, 512.0)[0] - b;
        assert!(step10 < step8 / 3.0, "10 bit の 1 段は 8 bit の 1/4 ほど");
    }

    #[test]
    fn p010_の上位詰めの値が符号語に戻る() {
        let scale = YuvParams::texture_code_scale(10);
        let raw16 = 504u32 << 6;
        let code = raw16 as f32 / 65535.0 * scale;
        assert!((code - 504.0).abs() < 1e-3, "{code}");
    }

    #[test]
    fn 原色の係数は行列の定義に合う() {
        let p = YuvParams::new(Matrix::Bt709, true, 8, 255.0);
        // 全レンジの純赤: Y = Kr * 255、Cr = 255、Cb = 128 - (Kr/(1-Kb)) ... 係数で往復して 1 になる
        let (kr, kb) = (0.2126f32, 0.0722f32);
        let y = kr * 255.0;
        let cb = 128.0 + (0.0 - y / 255.0) / (2.0 * (1.0 - kb)) * 255.0;
        let cr = 128.0 + (1.0 - y / 255.0) / (2.0 * (1.0 - kr)) * 255.0;
        let rgb = p.to_linear(y, cb, cr);
        assert!(
            (rgb[0] - 1.0).abs() < 1e-3 && rgb[1] < 1e-3 && rgb[2] < 1e-3,
            "{rgb:?}"
        );
    }
}
