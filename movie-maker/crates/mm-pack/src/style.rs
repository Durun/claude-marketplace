//! 文字のスタイル（`styles.json`）。`text` の文字と、その下に敷くプレート（角丸の地、縁、ずらした影、傾き）を決める。

use serde::Deserialize;

/// 文字のスタイル 1 つ。位置はコンポジションの画素で、`anchor` はプレートの中のどこを `position` に合わせるか（0..1）。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextStyleSpec {
    /// 字の大きさ（画素）。
    pub size: f64,
    /// 字の色（theme の色の名前か 16 進）。
    pub color: String,
    /// プレートを置く位置。
    pub position: [f64; 2],
    /// プレートの中の基準点。`[0.5, 1.0]` は下の中央。
    #[serde(default)]
    pub anchor: [f64; 2],
    /// 折り返す幅（画素）。これより長い文は複数行にする。
    #[serde(default)]
    pub max_width: Option<f64>,
    /// 文字の下に敷くプレート。無ければ文字だけ。
    #[serde(default)]
    pub plate: Option<PlateSpec>,
    /// 傾き（度。正が時計回り）。プレートの中心で回す。
    #[serde(default)]
    pub tilt: f64,
    /// 出る動き（不透明度を 0 から 1 へ）の長さ（秒）。
    #[serde(default)]
    pub fade_in: f64,
    /// 消える動きの長さ（秒）。
    #[serde(default)]
    pub fade_out: f64,
    /// パックの表記の置き換え（`corrections.json`）を文に適用する。
    #[serde(default)]
    pub corrections: bool,
}

/// 文字の下に敷くプレート。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlateSpec {
    /// 地の色。
    pub color: String,
    /// 地の不透明度（0..1）。
    #[serde(default = "opaque")]
    pub opacity: f64,
    /// 文字の周りの余白（横、縦。画素）。
    pub padding: [f64; 2],
    /// 角の丸みの半径（画素）。
    #[serde(default)]
    pub radius: f64,
    /// 縁。
    #[serde(default)]
    pub border: Option<Stroke>,
    /// 右下へずらした影。
    #[serde(default)]
    pub shadow: Option<Shadow>,
}

/// 縁の色と太さ（画素）。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stroke {
    pub color: String,
    pub width: f64,
}

/// 影の色と、右下へのずらし（画素）。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shadow {
    pub color: String,
    pub offset: f64,
}

fn opaque() -> f64 {
    1.0
}
