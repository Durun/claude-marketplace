//! パラメータの指し方。レイヤーかカットの ID と、その中のパス。
//!
//! パスは `.` で区切る。レイヤーは `opacity`、`transform.position|scale|rotation|anchor`、
//! `crop.left|top|right|bottom`、`source.<名前>`、`effects.<効果の ID>.<名前>`、
//! `masks.<マスクの ID>.path|feather|opacity|rect.position|rect.size|rect.radius`、
//! `audio.gain_db|pan`、`reframe.<カットの ID>.position|scale|rotation|anchor`、`caption.position|width`。
//! カットは `source.<名前>`。ノードのパラメータの名前は `.` を含んでよい。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use mm_doc::{CutId, Layer, Param, Project};
use mm_types::{LayerId, MmError, ParamValue, Result};

use crate::ops::add_nesting_reach;
use crate::touches::Touches;

const TRANSFORM: &str = "transform";
const CROP: &str = "crop";
const SOURCE: &str = "source";
const EFFECTS: &str = "effects";
const MASKS: &str = "masks";
const AUDIO: &str = "audio";
const REFRAME: &str = "reframe";
const CAPTION: &str = "caption";

/// パラメータの指し方。`layer` か `cut` のどちらか 1 つ。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParamRef {
    /// レイヤーの ID（レイヤーのパラメータを指すとき）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerId>,
    /// カットの ID（カットのノードのパラメータを指すとき）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut: Option<CutId>,
    /// 持ち主の中のパス。
    pub path: String,
}

impl ParamRef {
    /// レイヤーのパラメータ。
    pub fn layer(layer: &LayerId, path: impl Into<String>) -> Self {
        Self {
            layer: Some(layer.clone()),
            cut: None,
            path: path.into(),
        }
    }

    /// カットのノードのパラメータ。
    pub fn cut(cut: &CutId, path: impl Into<String>) -> Self {
        Self {
            layer: None,
            cut: Some(cut.clone()),
            path: path.into(),
        }
    }

    /// 触る所（持ち主の ID）。
    pub fn touches(&self) -> Result<Touches> {
        match (&self.layer, &self.cut) {
            (Some(l), None) => Ok(Touches::of(l)),
            (None, Some(c)) => Ok(Touches::of(c)),
            _ => Err(MmError::InvalidArgument(
                "ParamRef は layer か cut のどちらか 1 つ".into(),
            )),
        }
    }

    /// 触る所。ノードのパラメータ（`source.*`、`effects.*`）は入れ子の参照先を変えうるので、
    /// 持ち主を含むコンポジションか sequence と、新しい値 `value` が指す先とその先の全部も含める。
    pub(crate) fn touches_in(
        &self,
        project: &Project,
        value: Option<&ParamValue>,
    ) -> Result<Touches> {
        let mut t = self.touches()?;
        let is_node_param = self.path.starts_with("source.") || self.path.starts_with("effects.");
        if !is_node_param {
            return Ok(t);
        }
        match (&self.layer, &self.cut) {
            (Some(l), _) => {
                t.add(&project.layer_index(l)?.0);
            }
            (_, Some(c)) => {
                t.add(&project.cut_index(c)?.0);
            }
            _ => {}
        }
        if let Some(target) = value.and_then(ParamValue::as_text) {
            add_nesting_reach(project, &Touches::from_iter([target.to_owned()]), &mut t);
        }
        Ok(t)
    }

    /// 指す場所をプロジェクトのデータの中から引く。
    pub(crate) fn resolve<'a>(&self, project: &'a mut Project) -> Result<ParamSlot<'a>> {
        self.touches()?;
        if let Some(cut) = &self.cut {
            let cut = project.cut_mut(cut)?;
            return match self.path.split_once('.') {
                Some((SOURCE, name)) => Ok(ParamSlot::Map(&mut cut.source.params, name.into())),
                _ => Err(unknown_path(&self.path)),
            };
        }
        let layer = project.layer_mut(self.layer.as_ref().expect("touches が確かめた"))?;
        resolve_layer(layer, &self.path)
    }
}

fn unknown_path(path: &str) -> MmError {
    MmError::NotFound(format!("パラメータのパス {path}"))
}

fn resolve_layer<'a>(layer: &'a mut Layer, path: &str) -> Result<ParamSlot<'a>> {
    let (head, rest) = path.split_once('.').unwrap_or((path, ""));
    match head {
        "opacity" if rest.is_empty() => Ok(ParamSlot::Fixed(&mut layer.opacity)),
        TRANSFORM => match rest {
            "position" => Ok(ParamSlot::Fixed(&mut layer.transform.position)),
            "scale" => Ok(ParamSlot::Fixed(&mut layer.transform.scale)),
            "rotation" => Ok(ParamSlot::Fixed(&mut layer.transform.rotation)),
            "anchor" => Ok(ParamSlot::Optional(&mut layer.transform.anchor)),
            _ => Err(unknown_path(path)),
        },
        CROP => match rest {
            "left" => Ok(ParamSlot::Optional(&mut layer.crop.left)),
            "top" => Ok(ParamSlot::Optional(&mut layer.crop.top)),
            "right" => Ok(ParamSlot::Optional(&mut layer.crop.right)),
            "bottom" => Ok(ParamSlot::Optional(&mut layer.crop.bottom)),
            _ => Err(unknown_path(path)),
        },
        REFRAME => resolve_reframe(layer, rest).ok_or_else(|| unknown_path(path)),
        CAPTION => resolve_caption(layer, rest, path),
        SOURCE if !rest.is_empty() => Ok(ParamSlot::Map(&mut layer.source.params, rest.into())),
        EFFECTS => resolve_effect(layer, rest).ok_or_else(|| unknown_path(path)),
        MASKS => resolve_mask(layer, rest).ok_or_else(|| unknown_path(path)),
        AUDIO => {
            let audio = layer
                .audio
                .as_mut()
                .ok_or_else(|| MmError::NotFound(format!("レイヤー {} の音の設定", layer.id)))?;
            match rest {
                "gain_db" => Ok(ParamSlot::Fixed(&mut audio.gain_db)),
                "pan" => Ok(ParamSlot::Fixed(&mut audio.pan)),
                _ => Err(unknown_path(path)),
            }
        }
        _ => Err(unknown_path(path)),
    }
}

fn resolve_effect<'a>(layer: &'a mut Layer, rest: &str) -> Option<ParamSlot<'a>> {
    let (id, name) = rest.split_once('.')?;
    if name.is_empty() {
        return None;
    }
    let effect = layer.effects.iter_mut().find(|e| e.id.as_str() == id)?;
    Some(ParamSlot::Map(&mut effect.node.params, name.into()))
}

fn resolve_caption<'a>(layer: &'a mut Layer, rest: &str, path: &str) -> Result<ParamSlot<'a>> {
    let id = layer.id.clone();
    let caption = layer
        .framing
        .caption
        .as_mut()
        .ok_or_else(|| MmError::NotFound(format!("レイヤー {id} の字幕の置き場所")))?;
    match rest {
        "position" => Ok(ParamSlot::Fixed(&mut caption.position)),
        "width" => Ok(ParamSlot::Fixed(&mut caption.width)),
        _ => Err(unknown_path(path)),
    }
}

fn resolve_reframe<'a>(layer: &'a mut Layer, rest: &str) -> Option<ParamSlot<'a>> {
    let (id, name) = rest.rsplit_once('.')?;
    let reframe = layer
        .framing
        .cuts
        .iter_mut()
        .find(|r| r.cut.as_str() == id)?;
    let t = &mut reframe.transform;
    match name {
        "position" => Some(ParamSlot::Fixed(&mut t.position)),
        "scale" => Some(ParamSlot::Fixed(&mut t.scale)),
        "rotation" => Some(ParamSlot::Fixed(&mut t.rotation)),
        "anchor" => Some(ParamSlot::Optional(&mut t.anchor)),
        _ => None,
    }
}

fn resolve_mask<'a>(layer: &'a mut Layer, rest: &str) -> Option<ParamSlot<'a>> {
    let (id, name) = rest.split_once('.')?;
    let mask = layer.masks.iter_mut().find(|m| m.id.as_str() == id)?;
    match name {
        "path" => Some(ParamSlot::Fixed(&mut mask.path)),
        "feather" => Some(ParamSlot::Fixed(&mut mask.feather)),
        "opacity" => Some(ParamSlot::Fixed(&mut mask.opacity)),
        "rect.position" => Some(ParamSlot::Fixed(&mut mask.rect.as_mut()?.position)),
        "rect.size" => Some(ParamSlot::Fixed(&mut mask.rect.as_mut()?.size)),
        "rect.radius" => Some(ParamSlot::Fixed(&mut mask.rect.as_mut()?.radius)),
        _ => None,
    }
}

/// パラメータの置き場所。必ず在るフィールド、無くてもよいフィールド、ノードのパラメータのマップの 1 項目。
pub(crate) enum ParamSlot<'a> {
    Fixed(&'a mut Param),
    Optional(&'a mut Option<Param>),
    Map(&'a mut BTreeMap<String, Param>, String),
}

impl ParamSlot<'_> {
    /// 今の値。無ければ `None`。
    pub fn get(&self) -> Option<&Param> {
        match self {
            Self::Fixed(p) => Some(p),
            Self::Optional(p) => p.as_ref(),
            Self::Map(m, name) => m.get(name),
        }
    }

    /// 今の値（書き換え用）。無ければ `NotFound`。
    pub fn get_mut(&mut self) -> Result<&mut Param> {
        let missing = || MmError::NotFound("パラメータが無い".into());
        match self {
            Self::Fixed(p) => Ok(p),
            Self::Optional(p) => p.as_mut().ok_or_else(missing),
            Self::Map(m, name) => m.get_mut(name.as_str()).ok_or_else(missing),
        }
    }

    /// 値を置く（無ければ作る）。
    pub fn set(&mut self, param: Param) {
        match self {
            Self::Fixed(p) => **p = param,
            Self::Optional(p) => **p = Some(param),
            Self::Map(m, name) => {
                m.insert(name.clone(), param);
            }
        }
    }

    /// 値を外す。必ず在るフィールドは外せない。
    pub fn remove(&mut self) -> Result<Param> {
        let removed = match self {
            Self::Fixed(_) => {
                return Err(MmError::InvalidArgument(
                    "必ず在るパラメータは外せない".into(),
                ))
            }
            Self::Optional(p) => p.take(),
            Self::Map(m, name) => m.remove(name.as_str()),
        };
        removed.ok_or_else(|| MmError::NotFound("パラメータが無い".into()))
    }
}
