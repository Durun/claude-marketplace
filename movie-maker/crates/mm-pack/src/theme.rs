//! 見た目の土台（`theme.json`）。色の名前、書体、動きの長さの上限、画面の大きさ。

use std::collections::BTreeMap;

use mm_types::{MmError, Result, Rgba};
use serde::Deserialize;

/// `theme.json` の中身。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    /// 色の名前 → `#RRGGBB` か `#RRGGBBAA`（sRGB）。文字のスタイルとマクロはこの名前で色を指す。
    pub colors: BTreeMap<String, String>,
    /// 書体の役割 → 書体の名前。
    #[serde(default)]
    pub fonts: BTreeMap<String, String>,
    pub motion: Motion,
    /// 本編の画面。
    pub canvas: Canvas,
    /// 縦の動画（ショート）の画面。
    #[serde(default)]
    pub short: Option<Canvas>,
}

/// 動きの長さの上限（秒）。文字のスタイルの出る・消える動きはこれを超えない。
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Motion {
    /// 出る動きの上限。
    pub in_max: f64,
    /// 消える動きの上限。
    pub out_max: f64,
}

/// 画面の大きさと fps。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl Theme {
    /// 色の指定（名前か `#RRGGBB`、`#RRGGBBAA`）を、線形の premultiplied の色にする。
    pub fn color(&self, spec: &str) -> Result<Rgba> {
        let hex = if spec.starts_with('#') {
            spec
        } else {
            self.colors
                .get(spec)
                .ok_or_else(|| MmError::NotFound(format!("theme の色 {spec}")))?
        };
        parse_hex(hex)
    }
}

/// `#RRGGBB` か `#RRGGBBAA`（sRGB、straight のアルファ）を線形の premultiplied にする。
pub(crate) fn parse_hex(hex: &str) -> Result<Rgba> {
    let bad = || MmError::InvalidArgument(format!("色は #RRGGBB か #RRGGBBAA: {hex}"));
    let digits = hex.strip_prefix('#').ok_or_else(bad)?;
    if !matches!(digits.len(), 6 | 8) || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).map_err(|_| bad());
    let alpha = if digits.len() == 8 { byte(6)? } else { u8::MAX };
    let unit = |b: u8| f32::from(b) / f32::from(u8::MAX);
    Ok(Rgba::from_straight(
        srgb_to_linear(unit(byte(0)?)),
        srgb_to_linear(unit(byte(2)?)),
        srgb_to_linear(unit(byte(4)?)),
        unit(alpha),
    ))
}

/// sRGB の符号化を外す（IEC 61966-2-1 の区分の式）。
fn srgb_to_linear(v: f32) -> f32 {
    const LINEAR_KNEE: f32 = 0.04045;
    if v <= LINEAR_KNEE {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        serde_json::from_value(serde_json::json!({
            "colors": {"ink": "#0B1230", "half": "#FFFFFF80"},
            "motion": {"in_max": 0.8, "out_max": 0.3},
            "canvas": {"width": 1920, "height": 1080, "fps": 30}
        }))
        .unwrap()
    }

    #[test]
    fn 名前と16進で色を引く() {
        let t = theme();
        let white = t.color("#FFFFFF").unwrap();
        assert!(white.max_diff(Rgba::WHITE) < 1e-6);
        let half = t.color("half").unwrap();
        assert!((half.a - 128.0 / 255.0).abs() < 1e-6);
        assert!((half.r - half.a).abs() < 1e-6, "premultiplied");
        let ink = t.color("ink").unwrap();
        assert!(ink.b > ink.r, "墨は青みの黒");
    }

    #[test]
    fn 知らない名前と壊れた16進を拒む() {
        let t = theme();
        assert!(matches!(t.color("pink"), Err(MmError::NotFound(_))));
        assert!(t.color("#12345").is_err());
        assert!(t.color("#GGGGGG").is_err());
    }
}
