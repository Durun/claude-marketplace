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
