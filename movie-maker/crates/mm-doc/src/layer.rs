//! レイヤーとその付属（変形、時間、重ね方、マスク、効果、音）。

mod shape;

pub use shape::{Crop, Fade, FadeCurve, MaskRect};

use serde::{Deserialize, Serialize};

use mm_types::{Id, LayerId, ParamValue, Rational, RationalTime, Result, TimeRange};

use crate::node::Node;
use crate::param::{Interpolation, Param};
use crate::sequence::CutId;

/// 効果のマーカー。
#[derive(Debug)]
pub enum EffectMarker {}
/// マスクのマーカー。
#[derive(Debug)]
pub enum MaskMarker {}

/// 効果の ID。操作の `touches` と `mm diff` が指す。
pub type EffectId = Id<EffectMarker>;
/// マスクの ID。
pub type MaskId = Id<MaskMarker>;

/// 速さの既定（等速）。serde の `default` に渡す形。
pub fn unit_speed() -> Rational {
    Rational::integer(1)
}

/// `true`。serde の `default` に渡す形。
pub fn default_true() -> bool {
    true
}

/// レイヤー。ノードを 1 つ持ち、時間と重ね方を足す。
///
/// コンポジションの時間軸での範囲は `[start, start + (out - in) / speed)`。
/// `time_remap` があっても範囲は変わらず、範囲の中の写像だけ変わる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Layer {
    pub id: LayerId,
    pub source: Node,
    /// コンポジションの時間軸での始まり。
    pub start: RationalTime,
    /// 素材の時間軸でのイン点。
    #[serde(rename = "in")]
    pub in_point: RationalTime,
    /// 素材の時間軸での出（含まない）。`in < out`。
    #[serde(rename = "out")]
    pub out_point: RationalTime,
    /// 再生の速さ。1 が等速。型が正を保つので 0 にならない。
    #[serde(default = "unit_speed")]
    pub speed: Rational,
    /// 時間の写像の上書き。あれば `in` と `speed` の代わりに使う。
    #[serde(default)]
    pub time_remap: Option<TimeRemap>,
    /// 親のレイヤー（同じコンポジションの中）。変形を親に続けて掛ける。
    #[serde(default)]
    pub parent: Option<LayerId>,
    /// 接続先のカット。カットがリップルで動くと一緒に動く。
    #[serde(default)]
    pub attach_to: Option<CutId>,
    #[serde(default)]
    pub blend: BlendMode,
    pub opacity: Param,
    /// 位置・拡大・回転・アンカー。レンダラーが合成で掛ける（ノードの種類には渡さない）。
    #[serde(default)]
    pub transform: Transform,
    /// 切り抜き。変形の前のレイヤーの画像に掛ける。
    #[serde(default, skip_serializing_if = "Crop::is_empty")]
    pub crop: Crop,
    #[serde(default)]
    pub masks: Vec<Mask>,
    #[serde(default)]
    pub track_matte: Option<TrackMatte>,
    /// 効果の列。上から順に掛ける。
    #[serde(default)]
    pub effects: Vec<Effect>,
    #[serde(default)]
    pub audio: Option<LayerAudio>,
    /// 無効にしたレイヤーは描かず、鳴らさない。
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 人が直した所。Claude の操作経路からの操作は拒否する。
    #[serde(default)]
    pub lock: bool,
    /// sequence のレイヤーで、このコンポジションでのカットごとの再フレーミングと字幕の置き場所。
    #[serde(default)]
    pub framing: Framing,
}

/// sequence のレイヤーの、コンポジションごとの見せ方。同じ素材を本編（16:9）とショート（9:16）で別の位置から切り出す。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Framing {
    /// カットごとの再フレーミング。カットの ID は重ならない。無いカットは変形なし。
    pub cuts: Vec<CutReframe>,
    /// 字幕の置き場所。`None` は字幕を描く側の既定。
    pub caption: Option<CaptionPlacement>,
}

impl Framing {
    /// カットの再フレーミング。
    pub fn reframe(&self, cut: &CutId) -> Option<&CutReframe> {
        self.cuts.iter().find(|r| &r.cut == cut)
    }

    /// 持っているパラメータの並び（検証が巡る）。
    pub fn params(&self) -> impl Iterator<Item = &Param> {
        self.cuts
            .iter()
            .flat_map(|r| r.transform.params())
            .chain(self.caption.iter().flat_map(CaptionPlacement::params))
    }
}

/// カット 1 つの再フレーミング。カットの画像（コンポジションの画布に縦横比を保って収めたもの）に掛ける変形。
/// キーフレームの時刻は素材の時刻（被写体の位置は素材の時刻で決まるので、カットを動かしても切り出しがずれない）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CutReframe {
    pub cut: CutId,
    pub transform: Transform,
}

/// 字幕の置き場所。値はコンポジションの画素。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CaptionPlacement {
    /// 字幕の帯の下端の中央（Vec2）。
    pub position: Param,
    /// 字幕の帯の幅（Number）。これを超える行は折り返す。
    pub width: Param,
}

impl CaptionPlacement {
    /// 持っているパラメータの並び。
    pub fn params(&self) -> impl Iterator<Item = &Param> {
        [&self.position, &self.width].into_iter()
    }
}

impl Layer {
    /// 既定の重ね方・等速・効果なしのレイヤー。残りは `..` で上書きする。
    pub fn new(
        id: impl Into<String>,
        source: Node,
        start: RationalTime,
        in_point: RationalTime,
        out_point: RationalTime,
    ) -> Self {
        Self {
            id: Id::new(id),
            source,
            start,
            in_point,
            out_point,
            speed: unit_speed(),
            time_remap: None,
            parent: None,
            attach_to: None,
            blend: BlendMode::Normal,
            opacity: Param::constant(ParamValue::Number(1.0)),
            transform: Transform::default(),
            crop: Crop::default(),
            masks: Vec::new(),
            track_matte: None,
            effects: Vec::new(),
            audio: None,
            enabled: true,
            lock: false,
            framing: Framing::default(),
        }
    }

    /// レイヤーが持つノードの根（source と効果）。
    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        std::iter::once(&self.source).chain(self.effects.iter().map(|e| &e.node))
    }

    /// 素材の時間軸での範囲 `[in, out)`。
    pub fn source_range(&self) -> Result<TimeRange> {
        TimeRange::new(self.in_point, self.out_point)
    }

    /// コンポジションの時間軸での長さ `(out - in) / speed`。
    pub fn duration(&self) -> Result<RationalTime> {
        self.out_point
            .checked_sub(self.in_point)?
            .div_by(self.speed)
    }

    /// コンポジションの時間軸での範囲。
    pub fn range(&self) -> Result<TimeRange> {
        TimeRange::from_start_duration(self.start, self.duration()?)
    }
}

/// タイムリマップ。レイヤーの始まりからの時刻（`time`）を素材の時刻（`source`）に変換する折れ線。
/// 最初より前と最後より後は端の値を保つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TimeRemap {
    pub keyframes: Vec<TimeKeyframe>,
}

/// タイムリマップのキーフレーム。補間は `Hold` と `Linear` だけ（時刻は有理数のまま変換する）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TimeKeyframe {
    /// レイヤーの始まりからの時刻。
    pub time: RationalTime,
    /// 素材の時刻。
    pub source: RationalTime,
    pub interpolation: Interpolation,
}

/// 合成モード。式は W3C Compositing and Blending Level 1。`Add` は線形の加算（linear dodge）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
    Add,
}

/// レイヤーの変形。値はコンポジションの画素と度。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Transform {
    /// 動かす量（Vec2）。
    pub position: Param,
    /// 倍率（Number か Vec2）。1 が等倍。
    pub scale: Param,
    /// 度。正が時計回り。
    pub rotation: Param,
    /// 回転と拡大の中心（Vec2）。`None` はレイヤーの画像の中心。
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

    /// 持っているパラメータの並び（検証と操作が巡る）。
    pub fn params(&self) -> impl Iterator<Item = &Param> {
        [&self.position, &self.scale, &self.rotation]
            .into_iter()
            .chain(self.anchor.iter())
    }
}

/// マスク。形は角丸の矩形（`rect`）か、ベジェの頂点の列（`path`。型だけで、レンダラーは描かない）。
/// 座標はレイヤーの座標で、変形と一緒に動く（After Effects のレイヤーのマスクと同じ）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Mask {
    pub id: MaskId,
    /// 頂点の列。キーフレームで形を動かせる。`rect` があれば使わない。
    #[serde(default = "shape::empty_path")]
    pub path: Param,
    /// 角丸の矩形。あれば形はこれで決まる。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<MaskRect>,
    #[serde(default)]
    pub mode: MaskMode,
    #[serde(default)]
    pub inverted: bool,
    /// ぼかしの幅（画素）。縁を中心にこの幅で 0 から 1 へ移る。
    #[serde(default = "shape::zero")]
    pub feather: Param,
    #[serde(default = "shape::one")]
    pub opacity: Param,
}

/// マスクの組み合わせ方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MaskMode {
    #[default]
    Add,
    Subtract,
    Intersect,
}

/// トラックマット。同じコンポジションの別のレイヤーのアルファか輝度で切り抜く。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TrackMatte {
    pub layer: LayerId,
    pub kind: MatteKind,
    #[serde(default)]
    pub inverted: bool,
}

/// マットに使う成分。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MatteKind {
    Alpha,
    Luma,
}

/// 効果。ノードの種類を 1 つ、レイヤーの画像を入力にして掛ける。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Effect {
    pub id: EffectId,
    pub node: Node,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// レイヤーの音の設定。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LayerAudio {
    /// 役割（`voice`、`bgm`、`se`）。ミキサーのバスとダッキングが見る。
    pub role: String,
    pub gain_db: Param,
    pub pan: Param,
    /// 鳴らさない。プレビューと書き出しの両方に効く。
    #[serde(default, skip_serializing_if = "is_false")]
    pub mute: bool,
    /// ソロ。同じコンポジションにソロのレイヤーが 1 つでもあれば、ソロでないレイヤーは鳴らさない。
    #[serde(default, skip_serializing_if = "is_false")]
    pub solo: bool,
    /// 先頭のフェード。音量に掛ける。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_in: Option<Fade>,
    /// 末尾のフェード。音量に掛ける。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_out: Option<Fade>,
}

impl LayerAudio {
    /// 役割 `role` の、0 dB・中央・ミュートもソロもフェードもない設定。
    pub fn new(role: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            gain_db: Param::constant(ParamValue::Number(0.0)),
            pan: Param::constant(ParamValue::Number(0.0)),
            mute: false,
            solo: false,
            fade_in: None,
            fade_out: None,
        }
    }
}

fn is_false(v: &bool) -> bool {
    !*v
}
