//! 動画オブジェクトの見本（`solid`、`footage`、`text`）。契約テストは `contract` にある。
//!
//! 見本の種類は、返す絵の大きさを `cx.size` にそろえる（描画器は (0, 0) に重ねるだけでよい）。

mod contract;

use std::collections::HashMap;
use std::sync::Arc;

use mm_audio_api::AudioBlock;
use mm_node_api::{AudioCx, FrameCx, FrameRef, NodeKind, NodeRegistry, Traits};
use mm_text_api::{TextAlign, TextStyle};
use mm_types::{
    Id, ParamSchema, ParamSpec, ParamType, ParamValue, Params, Rect, Result, Rgba, RgbaImage,
};

pub use contract::{contract, render_audio, ConstInputs, Harness, NodeFixture};

/// 見本の種類の版。
pub const SAMPLE_KIND_VERSION: u32 = 1;

fn migrate_same_version(kind: &dyn NodeKind, old: serde_json::Value, from: u32) -> Result<Params> {
    if from != kind.version() {
        return Err(mm_types::MmError::Unsupported(format!(
            "{} の版 {from} からの移行が無い",
            kind.id()
        )));
    }
    let params: Params = serde_json::from_value(old)?;
    kind.params().validate(&params)?;
    Ok(params)
}

/// 単色。
#[derive(Debug, Default)]
pub struct SolidKind;

impl NodeKind for SolidKind {
    fn id(&self) -> &'static str {
        "solid"
    }

    fn version(&self) -> u32 {
        SAMPLE_KIND_VERSION
    }

    fn params(&self) -> ParamSchema {
        ParamSchema::new(vec![ParamSpec::new(
            "color",
            ParamType::Color,
            ParamValue::Color(Rgba::WHITE),
        )])
    }

    fn traits(&self) -> Traits {
        Traits {
            time_varying: false,
            ..Traits::PIXEL
        }
    }

    fn migrate(&self, old: serde_json::Value, from: u32) -> Result<Params> {
        migrate_same_version(self, old, from)
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        let color = cx
            .params
            .get("color")
            .and_then(ParamValue::as_color)
            .unwrap_or(Rgba::WHITE);
        Ok(FrameRef::cpu(RgbaImage::filled(cx.size, color)))
    }
}

/// 撮影素材。`asset` の素材を描画器が開いて `cx.footage` に渡す。
#[derive(Debug, Default)]
pub struct FootageKind;

impl NodeKind for FootageKind {
    fn id(&self) -> &'static str {
        "footage"
    }

    fn version(&self) -> u32 {
        SAMPLE_KIND_VERSION
    }

    fn params(&self) -> ParamSchema {
        ParamSchema::new(vec![ParamSpec::new(
            "asset",
            ParamType::AssetRef,
            ParamValue::AssetRef(Id::new("")),
        )])
    }

    fn traits(&self) -> Traits {
        Traits {
            audio: true,
            ..Traits::PIXEL
        }
    }

    fn migrate(&self, old: serde_json::Value, from: u32) -> Result<Params> {
        migrate_same_version(self, old, from)
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        let Some(footage) = cx.footage.as_deref_mut() else {
            return Ok(FrameRef::Transparent { size: cx.size });
        };
        let frame = footage.frame_at(cx.source_time)?;
        let Some(image) = frame.image else {
            return Err(mm_types::MmError::Unsupported(
                "見本は CPU のコマだけ".into(),
            ));
        };
        let mut canvas = RgbaImage::transparent(cx.size);
        canvas.blit_over(&image, 0, 0, 1.0);
        Ok(FrameRef::cpu(canvas))
    }

    fn render_audio(&self, cx: &mut AudioCx<'_>) -> Result<AudioBlock> {
        match cx.footage.as_deref_mut() {
            Some(footage) => footage.audio_in(cx.source_range),
            None => Ok(AudioBlock::silence(
                cx.sample_rate,
                cx.channels,
                mm_node_api::audio_frames_in(cx.range, cx.sample_rate),
            )),
        }
    }
}

/// 文字。`position` に `size` の字で描く。
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
            align: TextAlign::parse(
                params
                    .get("align")
                    .and_then(ParamValue::as_text)
                    .unwrap_or("left"),
            ),
            ..TextStyle::default()
        }
    }

    fn position(params: &Params) -> [f64; 2] {
        params
            .get("position")
            .and_then(ParamValue::as_vec2)
            .unwrap_or([0.0, 0.0])
    }
}

impl NodeKind for TextKind {
    fn id(&self) -> &'static str {
        "text"
    }

    fn version(&self) -> u32 {
        SAMPLE_KIND_VERSION
    }

    fn params(&self) -> ParamSchema {
        ParamSchema::new(vec![
            ParamSpec::new("text", ParamType::Text, ParamValue::Text("Text".into())),
            ParamSpec::new(
                "size",
                ParamType::Number {
                    min: Some(1.0),
                    max: None,
                },
                ParamValue::Number(32.0),
            ),
            ParamSpec::new("color", ParamType::Color, ParamValue::Color(Rgba::WHITE)),
            ParamSpec::new("position", ParamType::Vec2, ParamValue::Vec2([0.0, 0.0])),
            ParamSpec::new(
                "max_width",
                ParamType::Number {
                    min: Some(0.0),
                    max: None,
                },
                ParamValue::Number(0.0),
            ),
            ParamSpec::new("align", ParamType::Text, ParamValue::Text("left".into())),
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
        let text = cx
            .params
            .get("text")
            .and_then(ParamValue::as_text)
            .unwrap_or("");
        let Ok(shaped) = cx.services.text_shaper.shape(text, &style) else {
            return Rect::EMPTY;
        };
        let raster = crate::text::raster_size(&shaped, cx.scale);
        let [x, y] = Self::position(cx.params);
        Rect::new(
            (x as f32 * cx.scale).floor(),
            (y as f32 * cx.scale).floor(),
            raster.width as f32,
            raster.height as f32,
        )
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        let style = Self::style(cx.params);
        let text = cx
            .params
            .get("text")
            .and_then(ParamValue::as_text)
            .unwrap_or("");
        let shaped = cx.services.text_shaper.shape(text, &style)?;
        let glyphs = cx
            .services
            .text_rasterizer
            .rasterize(&shaped, &style, cx.scale)?;
        let [x, y] = Self::position(cx.params);
        let mut canvas = RgbaImage::transparent(cx.size);
        canvas.blit_over(
            &glyphs,
            (x as f32 * cx.scale).floor() as i64,
            (y as f32 * cx.scale).floor() as i64,
            1.0,
        );
        Ok(FrameRef::cpu(canvas))
    }
}

/// 名前で引くだけの登録簿。テストと見本の描画器に使う。
#[derive(Default, Clone)]
pub struct MapRegistry {
    kinds: HashMap<&'static str, Arc<dyn NodeKind>>,
}

impl MapRegistry {
    /// 見本の 3 種類を入れた登録簿。
    pub fn with_samples() -> Self {
        let mut r = Self::default();
        r.insert(Arc::new(SolidKind));
        r.insert(Arc::new(FootageKind));
        r.insert(Arc::new(TextKind));
        r
    }

    /// 種類を足す。同じ名前は置き換える。
    pub fn insert(&mut self, kind: Arc<dyn NodeKind>) {
        self.kinds.insert(kind.id(), kind);
    }
}

impl NodeRegistry for MapRegistry {
    fn get(&self, kind: &str) -> Option<Arc<dyn NodeKind>> {
        self.kinds.get(kind).cloned()
    }

    fn kinds(&self) -> Vec<&'static str> {
        let mut v: Vec<_> = self.kinds.keys().copied().collect();
        v.sort_unstable();
        v
    }
}
