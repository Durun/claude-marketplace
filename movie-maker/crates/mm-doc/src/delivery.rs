//! レンダーターゲットの納品の設定。字幕とチャプターをどう添えるかと、ラウドネスの正規化の上限。

use serde::{Deserialize, Serialize};

/// 書き出しのトゥルーピークの上限（dBTP）の既定。EBU R128 の -1 dBTP で、AAC に落とすときの余白になる。
pub const DEFAULT_TRUE_PEAK_DBTP: f32 = -1.0;

/// 字幕の置き方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SubtitleOutput {
    /// 字幕を作らない。
    #[default]
    None,
    /// 書き出しのファイルの隣に `<名前>.srt` と `<名前>.vtt` を置く。
    Sidecar,
    /// MP4 に字幕のトラック（mov_text）として入れる。
    Embedded,
}

/// 納品の設定。字幕とチャプターの出力と、トゥルーピークの上限。
/// ラウドネスの目標は `RenderTarget::loudness_lufs`（YouTube は -14、放送は -23）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Delivery {
    /// 字幕の置き方。字幕はシーケンスのカットの文字起こしから作る。
    #[serde(default)]
    pub subtitles: SubtitleOutput,
    /// コンポジションのマーカーを MP4 のチャプターにし、YouTube の説明欄用の `<名前>.chapters.txt` を置く。
    #[serde(default)]
    pub chapters: bool,
    /// トゥルーピークの上限（dBTP）。`None` なら [`DEFAULT_TRUE_PEAK_DBTP`]。ラウドネスの正規化のときだけ効く。
    #[serde(default)]
    pub true_peak_dbtp: Option<f32>,
}

impl Delivery {
    /// 上限のトゥルーピーク（dBTP）。
    pub fn ceiling_dbtp(&self) -> f32 {
        self.true_peak_dbtp.unwrap_or(DEFAULT_TRUE_PEAK_DBTP)
    }
}
