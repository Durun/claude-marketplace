//! プロジェクトのデータの評価（純関数。GPU も FFmpeg も使わない）。
//!
//! - [`eval_param`][]: 時刻 t でのパラメータの値（ホールド、直線、ベジェ、イージング）。
//! - [`source_time`][] / [`comp_time`][]: コンポジションの時刻と素材の時刻の写像（速さ、タイムリマップ、入れ子）。
//! - [`place_cuts`][]: マグネティックシーケンスのカットの配置。
//! - [`evaluate_transform`][] / [`evaluate_audio`][] / [`evaluate_opacity`][]: レイヤーの付属の評価。
//! - [`evaluate_crop`][] / [`evaluate_mask`][] / [`fade_gain`][]: 切り抜き、マスク、音のフェードの評価。

mod clip;
mod param;
mod sequence;
mod time;
mod transform;

pub use clip::{
    evaluate_crop, evaluate_mask, fade_curve_gain, fade_gain, has_fades, EvaluatedCrop,
    EvaluatedMask, FADE_DB_FLOOR,
};
pub use param::{ease, eval_param, lerp_value, node_params_at, BezierCurve};
pub use sequence::{cut_at, cut_source_time, place_cuts, reframe_at, sequence_duration, PlacedCut};
pub use time::{
    comp_time, comp_time_along, remap_forward, remap_inverse, source_time, source_time_along,
};
pub use transform::{
    any_solo, audible, evaluate_audio, evaluate_opacity, evaluate_transform, EvaluatedAudio,
    EvaluatedTransform, DEFAULT_AUDIO_ROLE,
};
