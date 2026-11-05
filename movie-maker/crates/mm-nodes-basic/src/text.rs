use mm_node_api::{FrameCx, FrameRef, NodeKind, Traits};
use mm_text_api::TextStyle;
use mm_types::{
    ParamSchema, ParamSpec, ParamType, ParamValue, Params, Rect, Result, Rgba, RgbaImage,
};

use crate::util::migrate_same_version;
use crate::KIND_VERSION;

/// 文字。`position`（1 倍の画素）に `size` の字で描く。整形と描画は `Services` の口に任せ、
/// 倍率 `cx.scale` で描くので拡大してもぼやけない（`vector`）。
#[derive(Debug, Default)]
pub struct TextKind;

impl TextKind {
    fn style(params: &Params) -> TextStyle {
        let max_width = params.number_or("max_width", 0.0);
        TextStyle {
            size_px: params.number_or("size", 32.0) as f32,
            color: params
                .get("color")
                .and_then(ParamValue::as_color)
                .unwrap_or(Rgba::WHITE),
            max_width: (max_width > 0.0).then_some(max_width as f32),
            ..TextStyle::default()
        }
    }

    fn text(params: &Params) -> &str {
        params
            .get("text")
            .and_then(ParamValue::as_text)
            .unwrap_or("")
    }

    /// 倍率をかけた左上の画素。
    fn origin(params: &Params, scale: f32) -> (f32, f32) {
        let [x, y] = params
            .get("position")
            .and_then(ParamValue::as_vec2)
            .unwrap_or([0.0, 0.0]);
        ((x as f32 * scale).floor(), (y as f32 * scale).floor())
    }
}

impl NodeKind for TextKind {
    fn id(&self) -> &'static str {
        "text"
    }

    fn version(&self) -> u32 {
        KIND_VERSION
    }

    fn params(&self) -> ParamSchema {
        let number = |min| ParamType::Number {
            min: Some(min),
            max: None,
        };
        ParamSchema::new(vec![
            ParamSpec::new("text", ParamType::Text, ParamValue::Text("Text".into())),
            ParamSpec::new("size", number(1.0), ParamValue::Number(32.0)),
            ParamSpec::new("color", ParamType::Color, ParamValue::Color(Rgba::WHITE)),
            ParamSpec::new("position", ParamType::Vec2, ParamValue::Vec2([0.0, 0.0])),
            ParamSpec::new("max_width", number(0.0), ParamValue::Number(0.0)),
        ])
    }

    fn traits(&self) -> Traits {
        Traits {
            time_varying: false,
            vector: true,
            ..Traits::PIXEL
        }
    }

    fn migrate(&self, old: serde_json::Value, from: u32) -> Result<Params> {
        migrate_same_version(self, old, from)
    }

    fn region_of_definition(&self, cx: &FrameCx<'_>) -> Rect {
        let style = Self::style(cx.params);
        let Ok(shaped) = cx.services.text_shaper.shape(Self::text(cx.params), &style) else {
            return Rect::EMPTY;
        };
        let (x, y) = Self::origin(cx.params, cx.scale);
        Rect::new(
            x,
            y,
            (shaped.bounds.width * cx.scale).ceil(),
            (shaped.bounds.height * cx.scale).ceil(),
        )
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        let style = Self::style(cx.params);
        let shaped = cx
            .services
            .text_shaper
            .shape(Self::text(cx.params), &style)?;
        let glyphs = cx
            .services
            .text_rasterizer
            .rasterize(&shaped, &style, cx.scale)?;
        let (x, y) = Self::origin(cx.params, cx.scale);
        let mut canvas = RgbaImage::transparent(cx.size);
        canvas.blit_over(&glyphs, x as i64, y as i64, 1.0);
        Ok(FrameRef::cpu(canvas))
    }
}
