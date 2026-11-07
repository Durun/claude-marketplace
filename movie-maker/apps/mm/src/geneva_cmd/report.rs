//! 変換できなかった要素と、近似した要素の一覧。

use std::collections::BTreeMap;

use serde_json::{json, Value};

/// 変換できない・近似した要素の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Gap {
    /// クリップのマスク（角丸、楕円、画像の輝度）。
    Mask,
    /// クリップの効果（ぼかし）。
    Effects,
    /// 重ね方（`normal` 以外）。
    Blend,
    /// クリップのトランジション。
    Transition,
    /// CSS のアニメーション。
    Animation,
    /// `fit` の `cover` と `fill`。
    FitCoverFill,
    /// 回転。
    Rotation,
    /// 直線でないイージング。
    Easing,
    /// 映像と画像以外の source（`html`、`text`、`captions`、`color`、`composition`）。
    Source,
    /// 字幕のトラック、入れ子のコンポジション、複数の出力。
    TopLevel,
    /// 音のクリップの速さ。
    AudioSpeed,
}

impl Gap {
    fn describe(self) -> (&'static str, &'static str) {
        match self {
            Self::Mask => ("mask", "楕円と画像のマスクを除いた（角丸の矩形だけ描く）"),
            Self::Effects => ("effects", "クリップの効果を除いた"),
            Self::Blend => ("blend", "重ね方を normal にした"),
            Self::Transition => ("transition", "トランジションを除いた"),
            Self::Animation => ("animation", "CSS のアニメーションを除いた"),
            Self::FitCoverFill => ("fit", "cover と fill を contain にした"),
            Self::Rotation => ("rotation", "回転を除いた"),
            Self::Easing => ("ease", "直線でないイージングを直線で近似した"),
            Self::Source => ("source", "映像と画像以外の source を除いた"),
            Self::TopLevel => ("top-level", "字幕のトラック、入れ子、複数の出力を除いた"),
            Self::AudioSpeed => ("audio.speed", "音のクリップの速さを 1 にした"),
        }
    }
}

/// 種類ごとの数。
#[derive(Debug, Default)]
pub struct Report {
    counts: BTreeMap<Gap, usize>,
}

impl Report {
    /// 1 つ数える。
    pub fn add(&mut self, gap: Gap) {
        *self.counts.entry(gap).or_default() += 1;
    }

    /// 一覧の JSON（種類、数、扱い）。
    pub fn to_json(&self) -> Value {
        let items: Vec<Value> = self
            .counts
            .iter()
            .map(|(gap, n)| {
                let (what, note) = gap.describe();
                json!({"what": what, "count": n, "note": note})
            })
            .collect();
        Value::Array(items)
    }
}
