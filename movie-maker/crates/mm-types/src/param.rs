//! パラメータのスキーマと評価済みの値。UI の編集欄と MCP の引数はこのスキーマから作る。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::color::Rgba;
use crate::error::{MmError, Result};
use crate::id::AssetId;

/// パラメータの型。数の型だけ補間でき、それ以外はホールド。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParamType {
    Number {
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
    },
    Vec2,
    Vec3,
    Color,
    Bool,
    Text,
    Choice {
        options: Vec<String>,
    },
    AssetRef,
    Json,
}

impl ParamType {
    /// 時間で補間できる型か。
    pub fn is_interpolable(&self) -> bool {
        matches!(
            self,
            ParamType::Number { .. } | ParamType::Vec2 | ParamType::Vec3 | ParamType::Color
        )
    }
}

/// 評価済みのパラメータの値（時刻 t での値）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ParamValue {
    Number(f64),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    Color(Rgba),
    Bool(bool),
    Text(String),
    Choice(String),
    AssetRef(AssetId),
    Json(serde_json::Value),
}

impl ParamValue {
    /// 数として読む。
    pub fn as_number(&self) -> Option<f64> {
        match self {
            ParamValue::Number(v) => Some(*v),
            _ => None,
        }
    }

    /// 文字として読む。`Choice` も文字として読める。
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ParamValue::Text(s) | ParamValue::Choice(s) => Some(s),
            _ => None,
        }
    }

    /// 色として読む。
    pub fn as_color(&self) -> Option<Rgba> {
        match self {
            ParamValue::Color(c) => Some(*c),
            _ => None,
        }
    }

    /// 2 次元の数として読む。
    pub fn as_vec2(&self) -> Option<[f64; 2]> {
        match self {
            ParamValue::Vec2(v) => Some(*v),
            _ => None,
        }
    }

    /// 真偽として読む。
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ParamValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// 素材の参照として読む。
    pub fn as_asset(&self) -> Option<&AssetId> {
        match self {
            ParamValue::AssetRef(id) => Some(id),
            _ => None,
        }
    }

    /// この値が型に合うか。
    pub fn matches(&self, ty: &ParamType) -> bool {
        match (self, ty) {
            (ParamValue::Number(_), ParamType::Number { .. }) => true,
            (ParamValue::Vec2(_), ParamType::Vec2) => true,
            (ParamValue::Vec3(_), ParamType::Vec3) => true,
            (ParamValue::Color(_), ParamType::Color) => true,
            (ParamValue::Bool(_), ParamType::Bool) => true,
            (ParamValue::Text(_), ParamType::Text) => true,
            (ParamValue::Choice(c), ParamType::Choice { options }) => options.contains(c),
            (ParamValue::AssetRef(_), ParamType::AssetRef) => true,
            (ParamValue::Json(_), ParamType::Json) => true,
            _ => false,
        }
    }
}

/// 1 つのパラメータの定義。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamSpec {
    pub name: String,
    pub ty: ParamType,
    pub default: ParamValue,
}

impl ParamSpec {
    /// 名前、型、既定値から。
    pub fn new(name: impl Into<String>, ty: ParamType, default: ParamValue) -> Self {
        Self {
            name: name.into(),
            ty,
            default,
        }
    }
}

/// 動画オブジェクトの種類が公開するパラメータの一覧。順序は UI に出す順。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ParamSchema {
    pub params: Vec<ParamSpec>,
}

impl ParamSchema {
    /// 定義の列から。
    pub fn new(params: Vec<ParamSpec>) -> Self {
        Self { params }
    }

    /// 名前で引く。
    pub fn get(&self, name: &str) -> Option<&ParamSpec> {
        self.params.iter().find(|p| p.name == name)
    }

    /// 全部が既定値の `Params`。
    pub fn defaults(&self) -> Params {
        Params(
            self.params
                .iter()
                .map(|p| (p.name.clone(), p.default.clone()))
                .collect(),
        )
    }

    /// 値がスキーマに合うかを確かめる。知らない名前と型違いは `InvalidArgument`。
    pub fn validate(&self, params: &Params) -> Result<()> {
        for (name, value) in &params.0 {
            let spec = self
                .get(name)
                .ok_or_else(|| MmError::InvalidArgument(format!("知らないパラメータ {name}")))?;
            if !value.matches(&spec.ty) {
                return Err(MmError::InvalidArgument(format!(
                    "パラメータ {name} の型が合わない"
                )));
            }
        }
        Ok(())
    }
}

/// 評価済みのパラメータの組。名前順で持ち、同じ中身なら同じ順に並ぶ（キャッシュのキーに使う）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Params(pub BTreeMap<String, ParamValue>);

impl Params {
    /// 空。
    pub fn new() -> Self {
        Self::default()
    }

    /// 値を置く。
    pub fn set(&mut self, name: impl Into<String>, value: ParamValue) -> &mut Self {
        self.0.insert(name.into(), value);
        self
    }

    /// 値を引く。
    pub fn get(&self, name: &str) -> Option<&ParamValue> {
        self.0.get(name)
    }

    /// 数を引く。無ければ `fallback`。
    pub fn number_or(&self, name: &str, fallback: f64) -> f64 {
        self.get(name)
            .and_then(ParamValue::as_number)
            .unwrap_or(fallback)
    }

    /// スキーマの既定値で埋めた組。自分の値が優先。
    pub fn with_defaults(&self, schema: &ParamSchema) -> Params {
        let mut out = schema.defaults();
        for (k, v) in &self.0 {
            out.0.insert(k.clone(), v.clone());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> ParamSchema {
        ParamSchema::new(vec![
            ParamSpec::new(
                "opacity",
                ParamType::Number {
                    min: Some(0.0),
                    max: Some(1.0),
                },
                ParamValue::Number(1.0),
            ),
            ParamSpec::new("text", ParamType::Text, ParamValue::Text(String::new())),
        ])
    }

    #[test]
    fn 既定値で埋まる() {
        let mut p = Params::new();
        p.set("text", ParamValue::Text("hi".into()));
        let filled = p.with_defaults(&schema());
        assert_eq!(filled.number_or("opacity", 0.0), 1.0);
        assert_eq!(filled.get("text").unwrap().as_text(), Some("hi"));
    }

    #[test]
    fn 型違いは断る() {
        let mut p = Params::new();
        p.set("opacity", ParamValue::Text("x".into()));
        assert!(schema().validate(&p).is_err());
    }
}
