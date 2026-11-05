use mm_node_api::{FrameCx, FrameRef, NodeKind, Traits};
use mm_types::{ParamSchema, ParamSpec, ParamType, ParamValue, Params, Result};

use crate::util::migrate_same_version;
use crate::KIND_VERSION;

/// 中のコンポジションの ID を持つパラメータの名前。描画器が読む。
pub const COMPOSITION_PARAM: &str = "composition";

/// 入れ子のコンポジション。中身は文書（`Project`）にあり、種類の口からは見えないので、
/// 展開は描画器が行う。種類自身は透明を返し、描画器の外で単独に呼ばれても安全にする。
#[derive(Debug, Default)]
pub struct CompositionKind;

impl NodeKind for CompositionKind {
    fn id(&self) -> &'static str {
        "composition"
    }

    fn version(&self) -> u32 {
        KIND_VERSION
    }

    fn params(&self) -> ParamSchema {
        ParamSchema::new(vec![ParamSpec::new(
            COMPOSITION_PARAM,
            ParamType::Text,
            ParamValue::Text(String::new()),
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
        Ok(FrameRef::Transparent { size: cx.size })
    }
}
