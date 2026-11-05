//! 文書の形。縦の 1 本に要る最小（Project、Asset、Composition、Layer、Node、Param、RenderTarget）。
//!
//! キーフレームの補間と時間の写像は `mm-eval` の仕事で、ここは形と serde と版の移行を持つ。
//! 文書に浮動小数の秒を書かず、時刻は全部 [`RationalTime`]。
//!
//! 古い版の JSON は [`load`]（[`migrate`]）で今の版に上げてから読む。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use mm_types::{
    AssetId, ColorSpace, CompositionId, LayerId, MmError, ParamValue, Params, Rational,
    RationalTime, RenderTargetId, Result, TimeRange,
};

/// 文書のスキーマの版。形を変えたら上げ、`fixtures/doc/vN/` を凍結する。
///
/// - v1: 最初の形。レイヤーの変形は動画オブジェクトのパラメータ `transform.*` に書いていた。
/// - v2: `Layer.transform`（位置・拡大・回転・アンカー、キーフレームつき）。
pub const SCHEMA_VERSION: u32 = 2;

mod migrate;

pub use migrate::{load, migrate};

/// プロジェクト。成果物（レンダーターゲット）は 1:N。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub schema_version: u32,
    pub assets: Vec<Asset>,
    pub compositions: Vec<Composition>,
    pub render_targets: Vec<RenderTarget>,
}

impl Project {
    /// 空のプロジェクト。今の版。
    pub fn new() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            assets: Vec::new(),
            compositions: Vec::new(),
            render_targets: Vec::new(),
        }
    }

    /// 素材を ID で引く。
    pub fn asset(&self, id: &AssetId) -> Result<&Asset> {
        self.assets
            .iter()
            .find(|a| &a.id == id)
            .ok_or_else(|| MmError::NotFound(format!("asset {id}")))
    }

    /// コンポジションを ID で引く。
    pub fn composition(&self, id: &CompositionId) -> Result<&Composition> {
        self.compositions
            .iter()
            .find(|c| &c.id == id)
            .ok_or_else(|| MmError::NotFound(format!("composition {id}")))
    }

    /// 文書の不変条件を確かめる。素材とコンポジションの長さが負でなく、レイヤーの範囲
    /// （`in <= out`）が作れること。fps が正であることは [`Rational`] の型が保つ。
    pub fn validate(&self) -> Result<()> {
        for asset in &self.assets {
            if asset.duration.is_negative() {
                return Err(MmError::InvalidArgument(format!(
                    "素材 {} の長さが負",
                    asset.id
                )));
            }
        }
        for comp in &self.compositions {
            comp.range().map_err(|e| {
                MmError::InvalidArgument(format!("コンポジション {} の長さ: {e}", comp.id))
            })?;
            for layer in &comp.layers {
                layer.range().map_err(|e| {
                    MmError::InvalidArgument(format!("レイヤー {} の範囲: {e}", layer.id))
                })?;
            }
        }
        Ok(())
    }

    /// レンダーターゲットを ID で引く。
    pub fn render_target(&self, id: &RenderTargetId) -> Result<&RenderTarget> {
        self.render_targets
            .iter()
            .find(|t| &t.id == id)
            .ok_or_else(|| MmError::NotFound(format!("render target {id}")))
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

/// 素材。原本のパス、中身のハッシュ、色空間、時間軸。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub id: AssetId,
    pub path: String,
    pub content_hash: String,
    pub color_space: ColorSpace,
    pub duration: RationalTime,
    /// 固定フレームレートなら fps。VFR は `None`（PTS の索引で引く）。
    pub fps: Option<Rational>,
    /// 音と映像の頭のずれ。OBS の mkv で起きる。
    #[serde(default = "RationalTime::zero")]
    pub timebase_offset: RationalTime,
}

/// コンポジション。大きさ、fps、長さ、レイヤーの重なり。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Composition {
    pub id: CompositionId,
    pub width: u32,
    pub height: u32,
    pub fps: Rational,
    pub duration: RationalTime,
    /// 下から上の順。
    pub layers: Vec<Layer>,
}

impl Composition {
    /// コンポジション全体の範囲 `[0, duration)`。長さが負なら `InvalidArgument`。
    pub fn range(&self) -> Result<TimeRange> {
        TimeRange::new(RationalTime::ZERO, self.duration)
    }
}

/// レイヤー。動画オブジェクトを 1 つ持ち、時間と重ね方を足す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub source: Node,
    /// コンポジションの時間軸での始まり。
    pub start: RationalTime,
    /// 素材の時間軸での入り。
    #[serde(rename = "in")]
    pub in_point: RationalTime,
    /// 素材の時間軸での出（含まない）。
    #[serde(rename = "out")]
    pub out_point: RationalTime,
    pub opacity: Param,
    /// 位置・拡大・回転・アンカー。描画器が合成で掛ける（種類には渡さない）。
    #[serde(default)]
    pub transform: Transform,
    #[serde(default)]
    pub audio: Option<LayerAudio>,
    #[serde(default)]
    pub lock: bool,
}

/// レイヤーの変形。値はコンポジションの画素と度。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transform {
    /// 動かす量（Vec2）。
    pub position: Param,
    /// 倍率（Number か Vec2）。1 が等倍。
    pub scale: Param,
    /// 度。正が時計回り。
    pub rotation: Param,
    /// 回転と拡大の中心（Vec2）。`None` はレイヤーの絵の中心。
    pub anchor: Option<Param>,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: Param::constant(ParamValue::Vec2([0.0, 0.0])),
            scale: Param::constant(ParamValue::Number(1.0)),
            rotation: Param::constant(ParamValue::Number(0.0)),
            anchor: None,
        }
    }
}

impl Transform {
    /// 恒等か。
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }
}

impl Layer {
    /// コンポジションの時間軸での範囲。
    pub fn range(&self) -> Result<TimeRange> {
        TimeRange::from_start_duration(self.start, self.out_point.checked_sub(self.in_point)?)
    }

    /// コンポジションの時刻を素材の時刻に写す（速さ 1 のみ。タイムリマップは `mm-eval`）。
    pub fn to_source_time(&self, t: RationalTime) -> Result<RationalTime> {
        t.checked_sub(self.start)?.checked_add(self.in_point)
    }
}

/// レイヤーの音の設定。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerAudio {
    pub role: String,
    pub gain_db: Param,
    pub pan: Param,
}

/// 動画オブジェクト。種類の名前と版、パラメータ、入力。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub kind: String,
    pub version: u32,
    #[serde(default)]
    pub params: BTreeMap<String, Param>,
    #[serde(default)]
    pub inputs: Vec<Node>,
}

impl Node {
    /// パラメータ無しの種類。
    pub fn new(kind: impl Into<String>, version: u32) -> Self {
        Self {
            kind: kind.into(),
            version,
            params: BTreeMap::new(),
            inputs: Vec::new(),
        }
    }

    /// 固定値のパラメータを足す。
    pub fn with_param(mut self, name: impl Into<String>, value: ParamValue) -> Self {
        self.params.insert(name.into(), Param::constant(value));
        self
    }

    /// 時刻 t での全パラメータの値（ホールドの評価）。
    pub fn params_at(&self, t: RationalTime) -> Params {
        let mut out = Params::new();
        for (k, p) in &self.params {
            out.set(k.clone(), p.hold_value_at(t));
        }
        out
    }
}

/// キーフレームの補間。数の型だけ `Linear` と `Bezier` が効く。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    Hold,
    Linear,
    Bezier,
}

/// キーフレーム。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    pub time: RationalTime,
    pub value: ParamValue,
    pub interpolation: Interpolation,
}

/// パラメータ。固定の値か、キーフレームの列。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub value: ParamValue,
    #[serde(default)]
    pub keyframes: Vec<Keyframe>,
}

impl Param {
    /// 固定の値。
    pub fn constant(value: ParamValue) -> Self {
        Self {
            value,
            keyframes: Vec::new(),
        }
    }

    /// 時刻 t で、直前のキーフレームの値をそのまま返す（ホールド）。
    /// キーフレームが無いか t が最初のキーフレームより前なら `value`。補間は `mm-eval` が担う。
    pub fn hold_value_at(&self, t: RationalTime) -> ParamValue {
        self.keyframes
            .iter()
            .filter(|k| k.time <= t)
            .max_by_key(|k| k.time)
            .map(|k| k.value.clone())
            .unwrap_or_else(|| self.value.clone())
    }
}

/// レンダーターゲット。成果物の形式。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderTarget {
    pub id: RenderTargetId,
    pub composition: CompositionId,
    pub width: u32,
    pub height: u32,
    pub fps: Rational,
    /// エンコーダーの名前（`h264_nvenc`、`prores_ks`、見本は `sample`）。
    pub codec: String,
    pub output_path: String,
    #[serde(default)]
    pub loudness_lufs: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_types::Id;

    #[test]
    fn 文書は_json_を往復する() {
        let mut p = Project::new();
        p.compositions.push(Composition {
            id: Id::new("c1"),
            width: 320,
            height: 180,
            fps: Rational::integer(30),
            duration: RationalTime::seconds(2),
            layers: vec![Layer {
                id: Id::new("l1"),
                source: Node::new("solid", 1)
                    .with_param("color", ParamValue::Color(mm_types::Rgba::WHITE)),
                start: RationalTime::ZERO,
                in_point: RationalTime::ZERO,
                out_point: RationalTime::seconds(2),
                opacity: Param::constant(ParamValue::Number(1.0)),
                transform: Transform::default(),
                audio: None,
                lock: false,
            }],
        });
        let s = serde_json::to_string_pretty(&p).unwrap();
        let back: Project = serde_json::from_str(&s).unwrap();
        assert_eq!(p, back);
        assert!(s.contains("\"in\""));
    }

    #[test]
    fn ホールドは直前のキーフレーム() {
        let p = Param {
            value: ParamValue::Number(0.0),
            keyframes: vec![
                Keyframe {
                    time: RationalTime::seconds(1),
                    value: ParamValue::Number(1.0),
                    interpolation: Interpolation::Hold,
                },
                Keyframe {
                    time: RationalTime::seconds(2),
                    value: ParamValue::Number(2.0),
                    interpolation: Interpolation::Hold,
                },
            ],
        };
        assert_eq!(p.hold_value_at(RationalTime::ZERO).as_number(), Some(0.0));
        assert_eq!(
            p.hold_value_at(RationalTime::new(3, 2).unwrap())
                .as_number(),
            Some(1.0)
        );
        assert_eq!(
            p.hold_value_at(RationalTime::seconds(5)).as_number(),
            Some(2.0)
        );
    }
}
