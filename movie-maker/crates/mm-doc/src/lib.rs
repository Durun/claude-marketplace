//! プロジェクトのデータの形。Project、Asset、Composition、Layer、Node、Param、Sequence、RenderTarget。
//!
//! キーフレームの補間と時間の写像は `mm-eval` の仕事で、ここは形と serde とバージョンの移行と不変条件を持つ。
//! プロジェクトのデータに浮動小数の秒を書かず、時刻は全部 [`RationalTime`]。
//!
//! 古いバージョンの JSON は [`load`]（[`migrate`]）で今のバージョンに上げてから読む。
//! 保存の JSON をバイトで決めるため、ID で引く列は [`Project::normalize`] で ID の順に並べる。

use serde::{Deserialize, Serialize};

use mm_types::{
    AssetId, ColorSpace, CompositionId, Id, MmError, Rational, RationalTime, RenderTargetId,
    Result, TimeRange,
};

/// プロジェクトのデータの整数のスキーマのバージョン。並行の PR が連番を取り合うので v9 で止め、
/// 以降の形の変更は名前つきの移行（`src/migrations/`、[`migration_names`]）で足す。
///
/// - v1: 最初の形。レイヤーの変形はノードのパラメータ `transform.*` に書いていた。
/// - v2: `Layer.transform`（位置・拡大・回転・アンカー、キーフレームつき）。
/// - v3: 段階 1 の形。レイヤーの速さ・タイムリマップ・親・接続・合成モード・マスク・効果・有効、
///   キーフレームのイージングとベジェ、コンポジションのマーカー、`sequences`。
///   v3 のマーカーは同じ時刻を持てた。
/// - v4: マーカーの時刻が厳密な昇順（同じ時刻を持てない）。v3 からは同じ時刻の後のマーカーを 1/1000 秒ずつ後ろへずらして上げる。
/// - v5: `color.lut` の LUT は素材（`lut` の `AssetRef`）。v4 の `path` はその場所の素材（`contentHash` は空）に移す。
/// - v6: 素材の整理（`bins`: イン点・アウト点、選別、話題）と文字起こしの修正（`transcript_edits`）。v5 からは空で上げる。
/// - v7: レンダーターゲットの納品の設定（`delivery`: 字幕、チャプター、トゥルーピークの上限）。v6 からは既定（何も添えない）で上げる。
/// - v8: 本編とショート。カットの上書き（`Cut.overrides`）とショートの元の値（`Cut.origin_base`）、
///   sequence のレイヤーの再フレーミングと字幕の置き場所（`Layer.framing`）。v7 のショートのカットは今の元の値を元の値にし、
///   参照のショートの元と違う in/out は上書きに、素材・速さ・有効が元と違えばショートごと複製にして上げる。
/// - v9: レンダーターゲットの字幕の焼き込み（`delivery.burn_in_captions`）。v8 からは焼き込まない（偽）で上げる。
pub const SCHEMA_VERSION: u32 = 9;

mod bins;
mod delivery;
mod layer;
mod migrate;
mod migrate_lut;
mod migrate_named;
mod migrate_shorts;
mod mixer;
mod node;
pub mod otio;
mod param;
mod sequence;
mod shorts;
mod validate;

pub use bins::{
    Bins, Rating, Selection, SelectionId, SelectionMarker, SourceMarks, Topic, TopicId,
    TopicMarker, WordEdit, WordId, WordMarker,
};
pub use delivery::{Delivery, SubtitleOutput, DEFAULT_TRUE_PEAK_DBTP};
pub use layer::{
    BlendMode, CaptionPlacement, Crop, CutReframe, Effect, EffectId, EffectMarker, Fade, FadeCurve,
    Framing, Layer, LayerAudio, Mask, MaskId, MaskMarker, MaskMode, MaskRect, MatteKind,
    TimeKeyframe, TimeRemap, TrackMatte, Transform,
};
pub use migrate::{load, migrate};
pub use migrate_named::{copy_stamp, is_current, migration_names, version_stamp, MIGRATIONS_FIELD};
pub use mixer::{
    CompMixer, Ducking, MixerBus, DEFAULT_DUCK_ATTACK_MS, DEFAULT_DUCK_REDUCTION_DB,
    DEFAULT_DUCK_RELEASE_MS, DEFAULT_DUCK_THRESHOLD_DB,
};
pub use node::{
    Node, FOOTAGE_KIND, FOOTAGE_PARAM, GAP_KIND, NESTED_KIND, NESTED_PARAM, SEQUENCE_KIND,
    SEQUENCE_PARAM,
};
pub use param::{CubicBezier, Ease, Interpolation, Keyframe, Param};
pub use sequence::{
    Cut, CutBase, CutId, CutMarker, CutOverrides, OriginRelation, Sequence, SequenceId,
    SequenceMarker, SequenceOrigin, Transition,
};
pub use shorts::{OriginChange, ReferenceSnapshot, ShortUse};

/// マーカーの ID の型タグ。
#[derive(Debug)]
pub enum MarkerMarker {}
/// マーカーの ID。
pub type MarkerId = Id<MarkerMarker>;

/// プロジェクト。成果物（レンダーターゲット）は 1:N。
///
/// `assets`、`compositions`、`sequences`、`render_targets` は ID で引く列で、順に意味が無い。
/// [`Project::normalize`] が ID の順に並べ、[`load`] も通す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Project {
    pub schema_version: u32,
    pub assets: Vec<Asset>,
    pub compositions: Vec<Composition>,
    #[serde(default)]
    pub sequences: Vec<Sequence>,
    pub render_targets: Vec<RenderTarget>,
    /// 素材の整理（イン点・アウト点、選別、話題）。
    #[serde(default)]
    pub bins: Bins,
    /// 文字起こしの修正。素材と単語の ID の順。
    #[serde(default)]
    pub transcript_edits: Vec<WordEdit>,
    /// 済ませた名前つきの移行の名前。名前の順。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub migrations: Vec<String>,
}

impl Project {
    /// 空のプロジェクト。今のバージョン。
    pub fn new() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            assets: Vec::new(),
            compositions: Vec::new(),
            sequences: Vec::new(),
            render_targets: Vec::new(),
            bins: Bins::default(),
            transcript_edits: Vec::new(),
            migrations: migration_names(),
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

    /// sequence を ID で引く。
    pub fn sequence(&self, id: &SequenceId) -> Result<&Sequence> {
        self.sequences
            .iter()
            .find(|s| &s.id == id)
            .ok_or_else(|| MmError::NotFound(format!("sequence {id}")))
    }

    /// レンダーターゲットを ID で引く。
    pub fn render_target(&self, id: &RenderTargetId) -> Result<&RenderTarget> {
        self.render_targets
            .iter()
            .find(|t| &t.id == id)
            .ok_or_else(|| MmError::NotFound(format!("render target {id}")))
    }

    /// レイヤーを ID で引く（全コンポジションから）。
    pub fn layer(&self, id: &mm_types::LayerId) -> Result<(&Composition, &Layer)> {
        self.compositions
            .iter()
            .find_map(|c| c.layers.iter().find(|l| &l.id == id).map(|l| (c, l)))
            .ok_or_else(|| MmError::NotFound(format!("layer {id}")))
    }

    /// カットを ID で引く（全 sequence から）。
    pub fn cut(&self, id: &CutId) -> Result<(&Sequence, &Cut)> {
        self.sequences
            .iter()
            .find_map(|s| s.cuts.iter().find(|c| &c.id == id).map(|c| (s, c)))
            .ok_or_else(|| MmError::NotFound(format!("cut {id}")))
    }

    /// コンポジションを ID で引く（書き換え用）。
    pub fn composition_mut(&mut self, id: &CompositionId) -> Result<&mut Composition> {
        self.compositions
            .iter_mut()
            .find(|c| &c.id == id)
            .ok_or_else(|| MmError::NotFound(format!("composition {id}")))
    }

    /// sequence を ID で引く（書き換え用）。
    pub fn sequence_mut(&mut self, id: &SequenceId) -> Result<&mut Sequence> {
        self.sequences
            .iter_mut()
            .find(|s| &s.id == id)
            .ok_or_else(|| MmError::NotFound(format!("sequence {id}")))
    }

    /// レイヤーの置き場所（コンポジションの ID と、下から数えた位置）。
    pub fn layer_index(&self, id: &mm_types::LayerId) -> Result<(CompositionId, usize)> {
        self.compositions
            .iter()
            .find_map(|c| {
                c.layers
                    .iter()
                    .position(|l| &l.id == id)
                    .map(|i| (c.id.clone(), i))
            })
            .ok_or_else(|| MmError::NotFound(format!("layer {id}")))
    }

    /// レイヤーを ID で引く（書き換え用）。
    pub fn layer_mut(&mut self, id: &mm_types::LayerId) -> Result<&mut Layer> {
        self.compositions
            .iter_mut()
            .find_map(|c| c.layers.iter_mut().find(|l| &l.id == id))
            .ok_or_else(|| MmError::NotFound(format!("layer {id}")))
    }

    /// カットの置き場所（sequence の ID と、先頭から数えた位置）。
    pub fn cut_index(&self, id: &CutId) -> Result<(SequenceId, usize)> {
        self.sequences
            .iter()
            .find_map(|s| {
                s.cuts
                    .iter()
                    .position(|c| &c.id == id)
                    .map(|i| (s.id.clone(), i))
            })
            .ok_or_else(|| MmError::NotFound(format!("cut {id}")))
    }

    /// カットを ID で引く（書き換え用）。
    pub fn cut_mut(&mut self, id: &CutId) -> Result<&mut Cut> {
        self.sequences
            .iter_mut()
            .find_map(|s| s.cuts.iter_mut().find(|c| &c.id == id))
            .ok_or_else(|| MmError::NotFound(format!("cut {id}")))
    }

    /// ID で引く列を ID の順に並べる。保存の前に通すと JSON がバイトで決まる。
    /// レイヤーとカットとマーカーの順は意味を持つので触らない。
    pub fn normalize(&mut self) {
        self.assets.sort_by(|a, b| a.id.cmp(&b.id));
        self.compositions.sort_by(|a, b| a.id.cmp(&b.id));
        self.sequences.sort_by(|a, b| a.id.cmp(&b.id));
        self.render_targets.sort_by(|a, b| a.id.cmp(&b.id));
        self.bins.normalize();
        bins::normalize_edits(&mut self.transcript_edits);
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

/// 素材。原本のパス、コンテンツハッシュ、色空間、時間軸。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Asset {
    pub id: AssetId,
    pub path: String,
    pub content_hash: String,
    pub color_space: ColorSpace,
    pub duration: RationalTime,
    /// 固定フレームレートなら fps。VFR は `None`（PTS の索引で引く）。
    pub fps: Option<Rational>,
    /// 音と映像の先頭のずれ。OBS の mkv で起きる。
    #[serde(default = "RationalTime::zero")]
    pub timebase_offset: RationalTime,
}

/// コンポジション。大きさ、fps、長さ、レイヤーの重なり、マーカー。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Composition {
    pub id: CompositionId,
    pub width: u32,
    pub height: u32,
    pub fps: Rational,
    pub duration: RationalTime,
    /// 下から上の順。
    pub layers: Vec<Layer>,
    /// 時刻の順。
    #[serde(default)]
    pub markers: Vec<Marker>,
    /// ミキサーの設定。`None` は既定（全部 0 dB、ダッキングなし）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mixer: Option<CompMixer>,
}

impl Composition {
    /// レイヤーもマーカーも無い、ミキサーが既定のコンポジション。
    /// フィールドを足しても呼び出し側の literal が壊れないよう、作るときはこの関数を通す。
    pub fn new(
        id: CompositionId,
        width: u32,
        height: u32,
        fps: Rational,
        duration: RationalTime,
    ) -> Self {
        Self {
            id,
            width,
            height,
            fps,
            duration,
            layers: Vec::new(),
            markers: Vec::new(),
            mixer: None,
        }
    }

    /// レイヤー（下から上の順）を置き換える。
    #[must_use]
    pub fn with_layers(mut self, layers: Vec<Layer>) -> Self {
        self.layers = layers;
        self
    }

    /// マーカー（時刻の順）を置き換える。
    #[must_use]
    pub fn with_markers(mut self, markers: Vec<Marker>) -> Self {
        self.markers = markers;
        self
    }

    /// ミキサーの設定を置く。
    #[must_use]
    pub fn with_mixer(mut self, mixer: CompMixer) -> Self {
        self.mixer = Some(mixer);
        self
    }

    /// ミキサーの設定を `Option` のまま置き換える。
    #[must_use]
    pub fn with_mixer_opt(mut self, mixer: Option<CompMixer>) -> Self {
        self.mixer = mixer;
        self
    }

    /// コンポジション全体の範囲 `[0, duration)`。長さが負なら `InvalidArgument`。
    pub fn range(&self) -> Result<TimeRange> {
        TimeRange::new(RationalTime::ZERO, self.duration)
    }

    /// レイヤーを ID で引く。
    pub fn layer(&self, id: &mm_types::LayerId) -> Result<&Layer> {
        self.layers
            .iter()
            .find(|l| &l.id == id)
            .ok_or_else(|| MmError::NotFound(format!("layer {id}")))
    }
}

/// マーカー。YouTube のチャプターの書き出しに使う。`duration` が 0 なら点。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Marker {
    pub id: MarkerId,
    pub time: RationalTime,
    #[serde(default = "RationalTime::zero")]
    pub duration: RationalTime,
    pub name: String,
}

/// レンダーターゲット。成果物の形式。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RenderTarget {
    pub id: RenderTargetId,
    pub composition: CompositionId,
    pub width: u32,
    pub height: u32,
    pub fps: Rational,
    /// エンコーダーの名前（`h264_nvenc`、`prores_ks`、モックは `sample`）。
    pub codec: String,
    pub output_path: String,
    #[serde(default)]
    pub loudness_lufs: Option<f32>,
    /// 字幕、チャプター、トゥルーピークの上限。
    #[serde(default)]
    pub delivery: Delivery,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_types::ParamValue;

    fn comp(id: &str, layers: Vec<Layer>) -> Composition {
        Composition::new(
            Id::new(id),
            320,
            180,
            Rational::integer(30),
            RationalTime::seconds(2),
        )
        .with_layers(layers)
    }

    #[test]
    fn プロジェクトのデータは_json_を往復する() {
        let mut p = Project::new();
        let node =
            Node::new("solid", 1).with_param("color", ParamValue::Color(mm_types::Rgba::WHITE));
        p.compositions.push(comp(
            "c1",
            vec![Layer::new(
                "l1",
                node,
                RationalTime::ZERO,
                RationalTime::ZERO,
                RationalTime::seconds(2),
            )],
        ));
        let s = serde_json::to_string_pretty(&p).unwrap();
        let back: Project = serde_json::from_str(&s).unwrap();
        assert_eq!(p, back);
        assert!(s.contains("\"in\""));
        assert!(s.contains("\"speed\""));
    }

    #[test]
    fn normalize_は_id_の順に並べる() {
        let mut p = Project::new();
        p.compositions.push(comp("b", vec![]));
        p.compositions.push(comp("a", vec![]));
        p.normalize();
        assert_eq!(p.compositions[0].id.as_str(), "a");
    }

    #[test]
    fn レイヤーの長さは速さで割る() {
        let mut l = Layer::new(
            "l",
            Node::new("solid", 1),
            RationalTime::seconds(1),
            RationalTime::ZERO,
            RationalTime::seconds(3),
        );
        l.speed = Rational::new(3, 2).unwrap();
        assert_eq!(l.duration().unwrap(), RationalTime::seconds(2));
        assert_eq!(l.range().unwrap().end, RationalTime::seconds(3));
    }
}
