use mm_node_api::{FrameCx, FrameRef, NodeKind, Traits};
use mm_types::{ParamSchema, ParamSpec, ParamType, ParamValue, Params, Result, Rgba, RgbaImage};

use crate::util::migrate_same_version;
use crate::KIND_VERSION;

/// 単色。`color` で画面全体を塗る。
#[derive(Debug, Default)]
pub struct SolidKind;

impl NodeKind for SolidKind {
    fn id(&self) -> &'static str {
        "solid"
    }

    fn version(&self) -> u32 {
        KIND_VERSION
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
