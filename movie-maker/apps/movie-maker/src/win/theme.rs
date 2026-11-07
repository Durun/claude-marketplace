//! サーフェスの配色。WebView と同じ `ui/theme` の theme.json の色を、Timeline の色に使う。
//! サーフェスは映像の周りなので、ダークのモードを使う。

use mm_ui_layout::palette::{parse_hex, Palette};
use serde_json::Value;

/// WebView と共有するテーマ。ビルドの時点で埋め込み、起動の場所に依らない。
const THEME_JSON: &str = include_str!("../../../../ui/theme/theme.json");
/// サーフェスに使うモード。
const MODE: &str = "dark";

/// theme.json のダークのモードから作った Timeline の色。読めなければ既定の色。
pub fn surface_palette() -> Palette {
    palette_from(THEME_JSON, MODE).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "theme.json を読めないので既定の色にする");
        Palette::DEFAULT
    })
}

fn palette_from(json: &str, mode: &str) -> Result<Palette, String> {
    let theme: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let colors = theme
        .pointer(&format!("/modes/{mode}/color"))
        .ok_or_else(|| format!("モード {mode} の色が無い"))?;
    Ok(Palette::themed(|name| {
        colors.get(name).and_then(Value::as_str).and_then(parse_hex)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ダークのモードの_surface_の色を使う() {
        let p = palette_from(THEME_JSON, MODE).expect("読める");
        let theme: Value = serde_json::from_str(THEME_JSON).unwrap();
        let surface = theme["modes"]["dark"]["color"]["surface"].as_str().unwrap();
        assert_eq!(Some(p.panel), parse_hex(surface));
        assert_ne!(p, Palette::DEFAULT);
    }

    #[test]
    fn 無いモードはエラー() {
        assert!(palette_from(THEME_JSON, "sepia").is_err());
    }
}
