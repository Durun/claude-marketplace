use mm_node_api::{FrameCx, FrameRef, NodeKind, Traits};
use mm_types::{ParamSchema, ParamSpec, ParamType, ParamValue, Params, Result};

use crate::util::migrate_same_version;
use crate::KIND_VERSION;

/// シーケンスの ID を持つパラメータの名前。レンダラーが読む。
pub const SEQUENCE_PARAM: &str = "sequence";

/// マグネティックシーケンス。カットの並びはプロジェクトのデータ（`Project.sequences`）にあり、種類のインターフェースからは
/// 見えないので、展開はレンダラーが行う。種類自身は透明を返し、レンダラーの外で単独に呼ばれても安全にする。
#[derive(Debug, Default)]
pub struct SequenceKind;

impl NodeKind for SequenceKind {
    fn id(&self) -> &'static str {
        "sequence"
    }

    fn version(&self) -> u32 {
        KIND_VERSION
    }

    fn params(&self) -> ParamSchema {
        ParamSchema::new(vec![ParamSpec::new(
            SEQUENCE_PARAM,
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
