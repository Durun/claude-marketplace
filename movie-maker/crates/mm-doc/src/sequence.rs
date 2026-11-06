//! マグネティックシーケンス（`sequence`）。カットの位置は前のカットのアウト点から決まり、プロジェクトのデータには書かない。
//! 配置の計算は `mm-eval`。

use serde::{Deserialize, Serialize};

use mm_types::{Id, Rational, RationalTime, Result, TimeRange};

use crate::layer::{default_true, unit_speed};
use crate::node::Node;

/// sequence のマーカー。
#[derive(Debug)]
pub enum SequenceMarker {}
/// カットのマーカー。
#[derive(Debug)]
pub enum CutMarker {}

/// sequence の ID。
pub type SequenceId = Id<SequenceMarker>;
/// カットの ID。`attachTo` と操作の `touches` が指す。
pub type CutId = Id<CutMarker>;

/// マグネティックシーケンス。OTIO の `Track` に対応づけられる形。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Sequence {
    pub id: SequenceId,
    /// 先頭から順。
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
    /// 素材の時間軸でのイン点。
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
    /// 参照のショートで、元のカットから上書きした所。上書きしていない所は元のカットに従う。
    #[serde(default)]
    pub overrides: CutOverrides,
    /// ショートで、分けたときの元のカットの値。元が変わったか（参照なら元を割ったか）の比べ先。
    #[serde(default)]
    pub origin_base: Option<CutBase>,
}

/// 参照のショートのカットで上書きした所。真の所は元のカットに従わない。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CutOverrides {
    #[serde(rename = "in")]
    pub in_point: bool,
    #[serde(rename = "out")]
    pub out_point: bool,
}

impl CutOverrides {
    /// どこかを上書きしているか。
    pub fn any(self) -> bool {
        self.in_point || self.out_point
    }
}

/// 元のカットの、ショートが従う・比べる値（素材、イン点、アウト点、速さ、有効）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CutBase {
    pub source: Node,
    #[serde(rename = "in")]
    pub in_point: RationalTime,
    #[serde(rename = "out")]
    pub out_point: RationalTime,
    pub speed: Rational,
    pub enabled: bool,
}

impl CutBase {
    /// カットの今の値。
    pub fn of(cut: &Cut) -> Self {
        Self {
            source: cut.source.clone(),
            in_point: cut.in_point,
            out_point: cut.out_point,
            speed: cut.speed,
            enabled: cut.enabled,
        }
    }

    /// `other` と違うフィールドの名前（JSON の名前）。
    pub fn changed_fields(&self, other: &CutBase) -> Vec<&'static str> {
        let pairs = [
            ("source", self.source != other.source),
            ("in", self.in_point != other.in_point),
            ("out", self.out_point != other.out_point),
            ("speed", self.speed != other.speed),
            ("enabled", self.enabled != other.enabled),
        ];
        pairs
            .into_iter()
            .filter_map(|(name, changed)| changed.then_some(name))
            .collect()
    }
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
            overrides: CutOverrides::default(),
            origin_base: None,
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

    /// シーケンスの上での長さ `(out - in) / speed`。
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
