//! 磁気の並び（`sequence`）。カットの位置は前のカットの尻から決まり、文書には書かない。
//! 配置の計算は `mm-eval`。

use serde::{Deserialize, Serialize};

use mm_types::{Id, Rational, RationalTime, Result, TimeRange};

use crate::layer::{default_true, unit_speed};
use crate::node::Node;

/// sequence の印。
#[derive(Debug)]
pub enum SequenceMarker {}
/// カットの印。
#[derive(Debug)]
pub enum CutMarker {}

/// sequence の ID。
pub type SequenceId = Id<SequenceMarker>;
/// カットの ID。`attachTo` と操作の `touches` が指す。
pub type CutId = Id<CutMarker>;

/// 磁気の並び。OTIO の `Track` に写せる形。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Sequence {
    pub id: SequenceId,
    /// 頭から順。
    pub cuts: Vec<Cut>,
    /// ショートが本編を参照・複製した元。本編なら `None`。
    #[serde(default)]
    pub origin: Option<SequenceOrigin>,
}

/// カット。素材の範囲を 1 つ切り出す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Cut {
    pub id: CutId,
    pub source: Node,
    /// 素材の時間軸での入り。
    #[serde(rename = "in")]
    pub in_point: RationalTime,
    /// 素材の時間軸での出（含まない）。`in < out`。
    #[serde(rename = "out")]
    pub out_point: RationalTime,
    #[serde(default = "unit_speed")]
    pub speed: Rational,
    /// 外したカットは消さずに残し、配置から除く。
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 前のカットからこのカットへのトランジション。境目をまたいで掛け、配置は変えない。
    #[serde(default)]
    pub transition_in: Option<Transition>,
    /// 参照・複製の元のカット。ショートで in/out を上書きしたときの対応に使う。
    #[serde(default)]
    pub origin: Option<CutId>,
}

impl Cut {
    /// 等速で有効なカット。
    pub fn new(
        id: impl Into<String>,
        source: Node,
        in_point: RationalTime,
        out_point: RationalTime,
    ) -> Self {
        Self {
            id: Id::new(id),
            source,
            in_point,
            out_point,
            speed: unit_speed(),
            enabled: true,
            transition_in: None,
            origin: None,
        }
    }

    /// カットが持つノードの根（source とトランジション）。
    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        std::iter::once(&self.source).chain(self.transition_in.iter().map(|t| &t.node))
    }

    /// 素材の時間軸での範囲 `[in, out)`。
    pub fn source_range(&self) -> Result<TimeRange> {
        TimeRange::new(self.in_point, self.out_point)
    }

    /// 並びの上での長さ `(out - in) / speed`。
    pub fn duration(&self) -> Result<RationalTime> {
        self.out_point
            .checked_sub(self.in_point)?
            .div_by(self.speed)
    }
}

/// カットの間のトランジション。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Transition {
    pub node: Node,
    pub duration: RationalTime,
}

/// ショートと本編の関係。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SequenceOrigin {
    pub sequence: SequenceId,
    pub relation: OriginRelation,
}

/// 参照（本編を直すとショートも変わる）か、複製（分岐。元が変わったら知らせる）か。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum OriginRelation {
    Reference,
    Fork,
}
