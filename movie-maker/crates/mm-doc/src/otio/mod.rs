//! OpenTimelineIO（`.otio` の JSON）と `sequence` の変換。対応表は `docs/design/07-standards.md`。
//!
//! 1 つの sequence が OTIO の `Timeline` の映像トラック 1 本になる。OTIO に無いもの（カットの ID、素材のノード、
//! ショートの上書き、sequence を置いたコンポジションなど）は、各オブジェクトの `metadata["movie-maker"]` に入れて
//! 往復で失わない。メタデータが無い（ほかのツールが書いた）ファイルも、OTIO の項目だけで読める。
//!
//! 形は変換だけで、プロジェクトへの反映は操作（`addAsset`、`addSequence`、`addComposition`）が受け持つ。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use mm_types::{Rational, RationalTime};

use crate::sequence::{CutBase, CutId, CutOverrides, Transition};
use crate::{Composition, Layer, Node, SEQUENCE_KIND, SEQUENCE_PARAM};

mod export;
mod import;
mod import_markers;
mod import_track;
mod time;

pub use export::export_sequence;
pub use import::{import_timeline, ImportOptions, Imported};

/// OTIO の `metadata` に movie-maker の値を入れるキー。
pub const NAMESPACE: &str = "movie-maker";
/// メタデータの形のバージョン。
pub const META_VERSION: u32 = 1;

/// カットの OTIO に無い値と、OTIO の値の厳密なコピー。
///
/// OTIO の値（`source_range`、`LinearTimeWarp`）がコピーと食い違うときは、別のツールが編集したとみなして OTIO の値に従う。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CutMeta {
    pub id: String,
    #[serde(rename = "in")]
    pub in_point: RationalTime,
    #[serde(rename = "out")]
    pub out_point: RationalTime,
    pub speed: Rational,
    pub enabled: bool,
    /// 素材の撮影素材のノードだけで済むときは無い。
    #[serde(default)]
    pub node: Option<Node>,
    #[serde(default)]
    pub transition_in: Option<Transition>,
    #[serde(default)]
    pub origin: Option<CutId>,
    #[serde(default)]
    pub overrides: CutOverrides,
    #[serde(default)]
    pub origin_base: Option<CutBase>,
}

/// トラックの OTIO に無い値。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TrackMeta {
    pub version: u32,
    pub sequence: String,
    #[serde(default)]
    pub origin: Option<crate::SequenceOrigin>,
    /// sequence を置いたコンポジション（マーカーは OTIO のマーカーに出すので空）。
    #[serde(default)]
    pub composition: Option<Composition>,
}

/// `metadata["movie-maker"]` を `T` として読む。無ければ `Ok(None)`。あるのに読めないときは理由を `Err` で返し、
/// 呼ぶ側が警告にして OTIO の値で取り込む（新しい形や壊れた値を黙って捨てない）。
pub(crate) fn read_meta<T: for<'de> Deserialize<'de>>(
    object: &Value,
) -> std::result::Result<Option<T>, String> {
    let Some(value) = object.get("metadata").and_then(|m| m.get(NAMESPACE)) else {
        return Ok(None);
    };
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|e| format!("{NAMESPACE} のメタデータが読めないので OTIO の値で取り込む: {e}"))
}

/// `metadata` に `movie-maker` の値だけを入れた JSON。
pub(crate) fn meta_json<T: Serialize>(value: &T) -> serde_json::Result<Value> {
    Ok(serde_json::json!({ NAMESPACE: serde_json::to_value(value)? }))
}

/// コンポジションの中で、sequence `id` を置いた最初のレイヤー。
pub(crate) fn sequence_layer<'a>(comp: &'a Composition, id: &str) -> Option<&'a Layer> {
    comp.layers
        .iter()
        .find(|l| l.source.kind == SEQUENCE_KIND && l.source.text_param(SEQUENCE_PARAM) == Some(id))
}

/// OTIO の `OTIO_SCHEMA`（`Clip.2` など）からバージョンを除いた名前。
pub(crate) fn schema_name(object: &Value) -> Option<&str> {
    let full = object.get("OTIO_SCHEMA")?.as_str()?;
    Some(full.split('.').next().unwrap_or(full))
}
