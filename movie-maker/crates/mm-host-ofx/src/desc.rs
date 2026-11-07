//! プラグインの記述（describe の結果）。ホストの実行ファイルが作り、アプリ側が JSON で受け取る。
//! パラメータは OpenFX の記述子の形（[`ParamDesc`]）で持ち、ノードの種類に載せるときは
//! [`PluginDesc::param_schema`] で `ParamSchema` に変える。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use mm_types::{ParamSchema, ParamSpec, ParamType, ParamValue, Rgba};

/// このホストが描ける文脈（OpenFX の context）。前にあるものを優先して選ぶ。
pub const RENDERABLE_CONTEXTS: [&str; 2] = [
    "OfxImageEffectContextFilter",
    "OfxImageEffectContextGenerator",
];

/// 1 つのバンドル（`<名前>.ofx.bundle`）を describe した結果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BundleDesc {
    pub bundle: PathBuf,
    /// 読み込んだ共有ライブラリ（`Contents/<アーキテクチャ>/<名前>.ofx`）。
    pub binary: PathBuf,
    pub plugins: Vec<PluginDesc>,
    /// 読み込めなかったプラグイン（画像の効果でない API、`Load` や `Describe` の失敗）。
    #[serde(default)]
    pub skipped: Vec<SkippedPlugin>,
}

/// 読み込めなかったプラグイン。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkippedPlugin {
    /// `OfxGetPlugin` に渡した番号。
    pub index: usize,
    pub id: String,
    pub reason: String,
}

/// 1 つのプラグイン（画像の効果）の記述。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginDesc {
    /// `pluginIdentifier`（逆ドメインの名前）。
    pub id: String,
    /// `[major, minor]`。
    pub version: [u32; 2],
    pub label: String,
    #[serde(default)]
    pub grouping: String,
    #[serde(default)]
    pub description: String,
    /// プラグインが対応する文脈（`OfxImageEffectContextFilter` など）。
    pub contexts: Vec<String>,
    /// プラグインが対応する画素の深さ（`OfxBitDepthFloat` など）。
    pub pixel_depths: Vec<String>,
    /// このホストが使う文脈。`None` ならこのホストでは描けない（パラメータも空）。
    pub context: Option<String>,
    /// `context` で describe したパラメータ。順序はプラグインが定義した順。
    pub params: Vec<ParamDesc>,
}

/// OpenFX のパラメータの型（`OfxParamType*`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    Integer,
    Double,
    Boolean,
    Choice,
    StrChoice,
    Rgba,
    Rgb,
    #[serde(rename = "double2d")]
    Double2D,
    #[serde(rename = "integer2d")]
    Integer2D,
    #[serde(rename = "double3d")]
    Double3D,
    #[serde(rename = "integer3d")]
    Integer3D,
    String,
    Custom,
    Bytes,
    Group,
    Page,
    PushButton,
}

/// `OfxParamType*` の名前と型の対応。
const KIND_NAMES: [(&str, ParamKind); 17] = [
    ("OfxParamTypeInteger", ParamKind::Integer),
    ("OfxParamTypeDouble", ParamKind::Double),
    ("OfxParamTypeBoolean", ParamKind::Boolean),
    ("OfxParamTypeChoice", ParamKind::Choice),
    ("OfxParamTypeStrChoice", ParamKind::StrChoice),
    ("OfxParamTypeRGBA", ParamKind::Rgba),
    ("OfxParamTypeRGB", ParamKind::Rgb),
    ("OfxParamTypeDouble2D", ParamKind::Double2D),
    ("OfxParamTypeInteger2D", ParamKind::Integer2D),
    ("OfxParamTypeDouble3D", ParamKind::Double3D),
    ("OfxParamTypeInteger3D", ParamKind::Integer3D),
    ("OfxParamTypeString", ParamKind::String),
    ("OfxParamTypeCustom", ParamKind::Custom),
    ("OfxParamTypeBytes", ParamKind::Bytes),
    ("OfxParamTypeGroup", ParamKind::Group),
    ("OfxParamTypePage", ParamKind::Page),
    ("OfxParamTypePushButton", ParamKind::PushButton),
];

impl ParamKind {
    /// `OfxParamTypeDouble` などの名前から。知らない名前は `None`。
    pub fn from_ofx(name: &str) -> Option<Self> {
        KIND_NAMES.iter().find(|(n, _)| *n == name).map(|(_, k)| *k)
    }

    /// OpenFX の名前。
    pub fn ofx_name(self) -> &'static str {
        KIND_NAMES
            .iter()
            .find(|(_, k)| *k == self)
            .map_or("", |(n, _)| n)
    }

    /// 値の成分の数。値を持たない型（グループ、ページ、ボタン）は 0。
    pub fn dimension(self) -> usize {
        match self {
            Self::Integer | Self::Double | Self::Boolean | Self::Choice => 1,
            Self::StrChoice | Self::String | Self::Custom | Self::Bytes => 1,
            Self::Double2D | Self::Integer2D => 2,
            Self::Rgb | Self::Double3D | Self::Integer3D => 3,
            Self::Rgba => 4,
            Self::Group | Self::Page | Self::PushButton => 0,
        }
    }

    /// 成分が整数か（`paramGetValue` が `int*` を受ける型）。
    pub fn is_integer(self) -> bool {
        matches!(
            self,
            Self::Integer | Self::Boolean | Self::Choice | Self::Integer2D | Self::Integer3D
        )
    }

    /// 値が文字か（`paramGetValue` が `char**` を受ける型）。
    pub fn is_text(self) -> bool {
        matches!(self, Self::String | Self::Custom | Self::StrChoice)
    }
}

/// 1 つのパラメータの記述。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamDesc {
    pub name: String,
    pub kind: ParamKind,
    pub label: String,
    #[serde(default)]
    pub hint: String,
    /// 既定値。成分が 1 つなら数・真偽・文字、複数なら配列。`Choice` は選択肢の番号。
    pub default: serde_json::Value,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    /// `Choice` の選択肢。
    #[serde(default)]
    pub choices: Vec<String>,
    /// 属するグループの名前（`kOfxParamPropParent`）。
    #[serde(default)]
    pub parent: String,
    /// 利用者に見せない（`kOfxParamPropSecret`）。
    #[serde(default)]
    pub secret: bool,
}

impl ParamDesc {
    /// `ParamSpec` に変える。値を持たない型（グループ、ページ、ボタン、バイト列）は `None`。
    /// 整数の型も `Number` になる（`ParamType` に整数の区別が無い）。
    pub fn to_spec(&self) -> Option<ParamSpec> {
        let nums = self.default_numbers();
        let num = |i: usize| nums.get(i).copied().unwrap_or(0.0);
        let (ty, default) = match self.kind {
            ParamKind::Integer | ParamKind::Double => (
                ParamType::Number {
                    min: self.min,
                    max: self.max,
                },
                ParamValue::Number(num(0)),
            ),
            ParamKind::Boolean => (ParamType::Bool, ParamValue::Bool(num(0) != 0.0)),
            ParamKind::Choice => (
                ParamType::Choice {
                    options: self.choices.clone(),
                },
                ParamValue::Choice(self.choices.get(num(0) as usize).cloned()?),
            ),
            ParamKind::Rgba | ParamKind::Rgb => (ParamType::Color, ParamValue::Color(self.color())),
            ParamKind::Double2D | ParamKind::Integer2D => {
                (ParamType::Vec2, ParamValue::Vec2([num(0), num(1)]))
            }
            ParamKind::Double3D | ParamKind::Integer3D => {
                (ParamType::Vec3, ParamValue::Vec3([num(0), num(1), num(2)]))
            }
            ParamKind::String | ParamKind::Custom | ParamKind::StrChoice => {
                let text = self.default.as_str().unwrap_or_default().to_string();
                (ParamType::Text, ParamValue::Text(text))
            }
            ParamKind::Bytes | ParamKind::Group | ParamKind::Page | ParamKind::PushButton => {
                return None
            }
        };
        Some(ParamSpec::new(self.name.clone(), ty, default))
    }

    fn default_numbers(&self) -> Vec<f64> {
        match &self.default {
            serde_json::Value::Array(items) => items.iter().filter_map(json_number).collect(),
            other => json_number(other).into_iter().collect(),
        }
    }

    fn color(&self) -> Rgba {
        let n = self.default_numbers();
        let at = |i: usize, fallback: f64| n.get(i).copied().unwrap_or(fallback) as f32;
        Rgba::new(at(0, 0.0), at(1, 0.0), at(2, 0.0), at(3, 1.0))
    }
}

fn json_number(v: &serde_json::Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_bool().map(f64::from))
}

impl PluginDesc {
    /// ノードの種類に載せるパラメータの一覧。隠しのパラメータと値を持たないものは除く。
    pub fn param_schema(&self) -> ParamSchema {
        ParamSchema::new(
            self.params
                .iter()
                .filter(|p| !p.secret)
                .filter_map(ParamDesc::to_spec)
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn param(kind: ParamKind, default: serde_json::Value) -> ParamDesc {
        ParamDesc {
            name: "p".into(),
            kind,
            label: "P".into(),
            hint: String::new(),
            default,
            min: Some(0.0),
            max: Some(2.0),
            choices: vec!["a".into(), "b".into()],
            parent: String::new(),
            secret: false,
        }
    }

    #[test]
    fn 型の名前は_ofx_の名前と往復する() {
        for (name, kind) in KIND_NAMES {
            assert_eq!(ParamKind::from_ofx(name), Some(kind));
            assert_eq!(kind.ofx_name(), name);
        }
        assert_eq!(ParamKind::from_ofx("OfxParamTypeNope"), None);
    }

    #[test]
    fn パラメータは_param_spec_に変わる() {
        let d = param(ParamKind::Double, serde_json::json!(0.5)).to_spec();
        assert_eq!(
            d.map(|s| (s.ty, s.default)),
            Some((
                ParamType::Number {
                    min: Some(0.0),
                    max: Some(2.0)
                },
                ParamValue::Number(0.5)
            ))
        );
        let c = param(ParamKind::Choice, serde_json::json!(1)).to_spec();
        assert_eq!(c.map(|s| s.default), Some(ParamValue::Choice("b".into())));
        let rgb = param(ParamKind::Rgb, serde_json::json!([1.0, 0.5, 0.0])).to_spec();
        assert_eq!(
            rgb.map(|s| s.default),
            Some(ParamValue::Color(Rgba::new(1.0, 0.5, 0.0, 1.0)))
        );
        assert_eq!(
            param(ParamKind::Group, serde_json::Value::Null).to_spec(),
            None
        );
    }
}
