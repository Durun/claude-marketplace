//! サーフェス上の UI の色。ダークな背景に、種類ごとの色の帯。

use crate::geom::Color;
use crate::timeline::ItemKind;

pub const BACKGROUND: Color = Color::hex(0x1b1d22);
pub const PANEL: Color = Color::hex(0x23262d);
pub const PROGRAM_BACKDROP: Color = Color::hex(0x0e0f12);
pub const RULER: Color = Color::hex(0x2c3039);
pub const ROW_EVEN: Color = Color::hex(0x20232a);
pub const ROW_ODD: Color = Color::hex(0x252932);
pub const TICK: Color = Color::hex(0x6b7280);
pub const LABEL: Color = Color::hex(0xb4bac6);
pub const OUTSIDE: Color = Color::hex(0x15161a).with_alpha(0.6);
pub const PLAYHEAD: Color = Color::hex(0xff5a4d);
pub const GUIDE: Color = Color::hex(0xffd24a);
pub const SELECTION: Color = Color::hex(0xffffff);
pub const BORDER: Color = Color::hex(0x3a3f4b);

/// サーフェスの UI の色の一式。既定は上の定数で、`ui/theme` の theme.json の色で差し替えられる。
/// 再生位置、スナップの案内線、帯の種類の色、Program の背景はテーマに無いので既定のまま。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub panel: Color,
    pub ruler: Color,
    pub row_even: Color,
    pub row_odd: Color,
    pub tick: Color,
    pub label: Color,
    pub outside: Color,
    pub playhead: Color,
    pub guide: Color,
    pub selection: Color,
}

impl Palette {
    /// 上の定数の色。
    pub const DEFAULT: Self = Self {
        panel: PANEL,
        ruler: RULER,
        row_even: ROW_EVEN,
        row_odd: ROW_ODD,
        tick: TICK,
        label: LABEL,
        outside: OUTSIDE,
        playhead: PLAYHEAD,
        guide: GUIDE,
        selection: SELECTION,
    };

    /// theme.json の 1 つのモードの `color` から。`color(name)` は色の名前（`bg`、`surface` など）の値を返す。
    /// 無い名前の色は既定のまま。
    pub fn themed(color: impl Fn(&str) -> Option<Color>) -> Self {
        let pick = |name: &str, fallback: Color| color(name).unwrap_or(fallback);
        let d = Self::DEFAULT;
        Self {
            panel: pick("surface", d.panel),
            ruler: pick("bg", d.ruler),
            row_even: pick("surface", d.row_even),
            row_odd: pick("bg", d.row_odd),
            tick: pick("border-strong", d.tick),
            label: pick("text-muted", d.label),
            outside: pick("bg", d.outside).with_alpha(d.outside.0[3]),
            selection: pick("text", d.selection),
            ..d
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// `#rrggbb` の色。形が違えば `None`。
pub fn parse_hex(text: &str) -> Option<Color> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    u32::from_str_radix(digits, 16).ok().map(Color::hex)
}

/// 帯の色。
pub fn item_color(kind: ItemKind) -> Color {
    match kind {
        ItemKind::Footage => Color::hex(0x3b78c8),
        ItemKind::Image => Color::hex(0x3fa66a),
        ItemKind::Solid => Color::hex(0x6e7686),
        ItemKind::Text => Color::hex(0xc8922f),
        ItemKind::Composition => Color::hex(0x8a5bc4),
        ItemKind::Other => Color::hex(0x5a6070),
    }
}

/// 明るくする（つまみ、ホバー）。
pub fn lighten(c: Color, amount: f32) -> Color {
    Color([
        c.0[0] + (1.0 - c.0[0]) * amount,
        c.0[1] + (1.0 - c.0[1]) * amount,
        c.0[2] + (1.0 - c.0[2]) * amount,
        c.0[3],
    ])
}
