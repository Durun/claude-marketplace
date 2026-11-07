//! パラメータ（`OfxParamHandle`、`OfxParamSetHandle` の中身）。C から呼ばれる関数は `param_suite.rs`。
//! 値は時刻ごとに持たない。アプリが時刻で評価した値を `RenderFrame` ごとに渡し、プラグインがどの時刻で
//! 読んでもその値を返す（キーフレームは 0 個に見える）。

use std::ffi::{c_int, CString};
use std::sync::{Arc, Mutex, MutexGuard};

use mm_host_ofx::desc::{ParamDesc, ParamKind};
use mm_types::ParamValue;

use super::ffi::*;
use super::names::*;
use super::props::{cstring, PropSet};

/// パラメータの今の値。
#[derive(Debug, Clone, PartialEq)]
pub enum Raw {
    Ints(Vec<c_int>),
    Doubles(Vec<f64>),
    Text(CString),
    Nothing,
}

/// 1 つのパラメータ（記述子かインスタンス）。
#[derive(Debug)]
pub struct Param {
    pub kind: ParamKind,
    pub props: PropSet,
    value: Mutex<Raw>,
}

impl Param {
    pub fn value(&self) -> MutexGuard<'_, Raw> {
        self.value.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn name(&self) -> String {
        self.props.string(PROP_NAME)
    }

    /// 既定値のプロパティから今の値を作る。
    fn reset_to_default(&self) {
        let raw = match (self.kind.dimension(), self.props.get(PARAM_DEFAULT)) {
            (0, _) | (_, None) => Raw::Nothing,
            (_, Some(super::props::Value::Int(v))) => Raw::Ints(v),
            (_, Some(super::props::Value::Double(v))) => Raw::Doubles(v),
            (_, Some(super::props::Value::Str(v))) => {
                Raw::Text(v.into_iter().next().unwrap_or_default())
            }
            (_, Some(super::props::Value::Ptr(_))) => Raw::Nothing,
        };
        *self.value() = raw;
    }

    /// アプリが評価した値を入れる。型と成分の数が合わなければ誤り。
    pub fn apply(&self, value: &ParamValue) -> Result<(), String> {
        let raw = self.convert(value).ok_or_else(|| {
            format!(
                "パラメータ {} の型 {:?} に {value:?} は入らない",
                self.name(),
                self.kind
            )
        })?;
        *self.value() = raw;
        Ok(())
    }

    fn convert(&self, value: &ParamValue) -> Option<Raw> {
        let dim = self.kind.dimension();
        let nums: Vec<f64> = match value {
            ParamValue::Number(x) => vec![*x],
            ParamValue::Bool(b) => vec![f64::from(u8::from(*b))],
            ParamValue::Vec2(v) => v.to_vec(),
            ParamValue::Vec3(v) => v.to_vec(),
            ParamValue::Color(c) if matches!(self.kind, ParamKind::Rgb | ParamKind::Rgba) => {
                [c.r, c.g, c.b, c.a][..dim]
                    .iter()
                    .map(|x| f64::from(*x))
                    .collect()
            }
            ParamValue::Color(_) => return None,
            ParamValue::Text(s) if self.kind.is_text() => return Some(Raw::Text(cstring(s))),
            ParamValue::Text(s) | ParamValue::Choice(s) => vec![self.choice_index(s)? as f64],
            ParamValue::AssetRef(_) | ParamValue::Json(_) => return None,
        };
        if nums.len() != dim || self.kind.is_text() {
            return None;
        }
        Some(if self.kind.is_integer() {
            Raw::Ints(nums.iter().map(|x| x.round() as c_int).collect())
        } else {
            Raw::Doubles(nums)
        })
    }

    fn choice_index(&self, label: &str) -> Option<usize> {
        (self.kind == ParamKind::Choice)
            .then(|| self.props.strings(PARAM_CHOICE_OPTION))?
            .iter()
            .position(|o| o == label)
    }

    /// describe の結果に載せる記述。
    pub fn desc(&self) -> ParamDesc {
        let ranged = !matches!(self.kind, ParamKind::Boolean | ParamKind::Choice);
        let bound = |name: &str| {
            let v = *self.props.numbers(name).first()?;
            (ranged && v.abs() < f64::from(c_int::MAX)).then_some(v)
        };
        ParamDesc {
            name: self.name(),
            kind: self.kind,
            label: self.props.string(PROP_LABEL),
            hint: self.props.string(PARAM_HINT),
            default: self.default_json(),
            min: bound(PARAM_MIN),
            max: bound(PARAM_MAX),
            choices: self.props.strings(PARAM_CHOICE_OPTION),
            parent: self.props.string(PARAM_PARENT),
            secret: self.props.int(PARAM_SECRET).unwrap_or(0) != 0,
        }
    }

    fn default_json(&self) -> serde_json::Value {
        use serde_json::Value as J;
        let items: Vec<J> = match self.props.get(PARAM_DEFAULT) {
            Some(super::props::Value::Int(v)) if self.kind == ParamKind::Boolean => {
                v.iter().map(|x| J::Bool(*x != 0)).collect()
            }
            Some(super::props::Value::Int(v)) => v.iter().map(|x| J::from(*x)).collect(),
            Some(super::props::Value::Double(v)) => v.iter().map(|x| J::from(*x)).collect(),
            Some(super::props::Value::Str(v)) => {
                v.iter().map(|s| J::from(s.to_string_lossy())).collect()
            }
            _ => Vec::new(),
        };
        match (self.kind.dimension(), items.len()) {
            (0, _) | (_, 0) => J::Null,
            (1, _) => items.into_iter().next().unwrap_or(J::Null),
            _ => J::Array(items),
        }
    }
}

/// 型ごとの既定のプロパティ。プラグインは describe でこれを書き換える。
fn default_props(kind: ParamKind, name: &str) -> PropSet {
    let p = PropSet::default();
    p.set_str(PROP_TYPE, &[TYPE_PARAMETER]);
    for key in [
        PROP_NAME,
        PROP_LABEL,
        PROP_SHORT_LABEL,
        PROP_LONG_LABEL,
        PARAM_SCRIPT_NAME,
    ] {
        p.set_str(key, &[name]);
    }
    p.set_str(PARAM_TYPE, &[kind.ofx_name()]);
    p.set_str(PARAM_HINT, &[""]);
    p.set_str(PARAM_PARENT, &[""]);
    p.set_int(PARAM_SECRET, &[0]);
    p.set_int(PARAM_ENABLED, &[1]);
    if kind.dimension() > 0 {
        set_value_defaults(&p, kind);
    }
    p
}

/// 値を持つ型の既定値と範囲。範囲の既定は型の全域（describe の結果では範囲なしに見せる）。
fn set_value_defaults(p: &PropSet, kind: ParamKind) {
    let dim = kind.dimension();
    p.set_int(PARAM_ANIMATES, &[1]);
    if kind.is_text() {
        p.set_str(PARAM_DEFAULT, &[""]);
    } else if kind == ParamKind::Choice {
        p.set_int(PARAM_DEFAULT, &[0]);
        p.set(PARAM_CHOICE_OPTION, super::props::Value::Str(Vec::new()));
    } else if kind.is_integer() {
        p.set_int(PARAM_DEFAULT, &vec![0; dim]);
        for (key, v) in [
            (PARAM_MIN, c_int::MIN),
            (PARAM_DISPLAY_MIN, c_int::MIN),
            (PARAM_MAX, c_int::MAX),
            (PARAM_DISPLAY_MAX, c_int::MAX),
        ] {
            p.set_int(key, &vec![v; dim]);
        }
    } else {
        p.set_double(PARAM_DEFAULT, &vec![0.0; dim]);
        for (key, v) in [
            (PARAM_MIN, -f64::MAX),
            (PARAM_DISPLAY_MIN, -f64::MAX),
            (PARAM_MAX, f64::MAX),
            (PARAM_DISPLAY_MAX, f64::MAX),
        ] {
            p.set_double(key, &vec![v; dim]);
        }
    }
}

/// パラメータの集合。プラグインに渡したハンドルが動かないよう、各パラメータを `Arc` で持つ。
#[derive(Debug, Default)]
pub struct ParamSet {
    pub props: PropSet,
    params: Mutex<Vec<Arc<Param>>>,
}

impl ParamSet {
    pub fn handle(&self) -> Handle {
        self as *const Self as Handle
    }

    fn list(&self) -> MutexGuard<'_, Vec<Arc<Param>>> {
        self.params.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 名前で引く。
    pub fn find(&self, name: &str) -> Option<Arc<Param>> {
        self.list().iter().find(|p| p.name() == name).cloned()
    }

    pub fn define(&self, kind: ParamKind, name: &str) -> Result<Handle, OfxStatus> {
        if self.find(name).is_some() {
            return Err(STAT_ERR_EXISTS);
        }
        let param = Arc::new(Param {
            kind,
            props: default_props(kind, name),
            value: Mutex::new(Raw::Nothing),
        });
        let props = param.props.handle();
        self.list().push(param);
        Ok(props)
    }

    /// 記述子から、既定値の入ったインスタンスの集合を作る。
    pub fn instantiate(&self) -> Self {
        let params = self
            .list()
            .iter()
            .map(|d| {
                let p = Arc::new(Param {
                    kind: d.kind,
                    props: d.props.duplicate(),
                    value: Mutex::new(Raw::Nothing),
                });
                p.props.set_str(PROP_TYPE, &[TYPE_PARAMETER_INSTANCE]);
                p.reset_to_default();
                p
            })
            .collect();
        Self {
            props: self.props.duplicate(),
            params: Mutex::new(params),
        }
    }

    /// describe の結果に載せる記述（定義した順）。
    pub fn descs(&self) -> Vec<ParamDesc> {
        self.list().iter().map(|p| p.desc()).collect()
    }
}
