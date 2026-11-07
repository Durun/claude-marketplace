//! 引数を型どおりに読み、手順の中の `{{…}}` を埋める。

use std::collections::BTreeMap;

use mm_types::{MmError, RationalTime, Result};
use serde_json::Value;

use crate::macros::{MacroDef, ParamType};
use crate::theme::Theme;

/// 小数の秒の桁の上限（`mm op` の時刻と同じ）。
const MAX_DECIMALS: usize = 6;

/// 型どおりに読んだ引数。
#[derive(Debug, Clone, PartialEq)]
pub enum ArgValue {
    Text(String),
    Time(RationalTime),
    Number(f64),
    /// 省いた任意の引数。
    Missing,
}

impl ArgValue {
    fn parse(ty: ParamType, raw: &str) -> Result<Self> {
        Ok(match ty {
            ParamType::Text | ParamType::Id => Self::Text(raw.to_string()),
            ParamType::Time => Self::Time(parse_time(raw)?),
            ParamType::Number => Self::Number(
                raw.trim()
                    .parse()
                    .map_err(|_| MmError::InvalidArgument(format!("数として読めない: {raw}")))?,
            ),
        })
    }

    /// 文字列の一部に埋めるときの形。時刻は `num/den`（[`parse_time`] がそのまま読める）。
    fn to_text(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Time(t) => format!("{}/{}", t.num(), t.den()),
            Self::Number(n) => number_json(*n).to_string(),
            Self::Missing => String::new(),
        }
    }

    /// 文字列の全体を置き換えるときの形。
    fn to_json(&self) -> Result<Value> {
        Ok(match self {
            Self::Text(s) => Value::String(s.clone()),
            Self::Time(t) => serde_json::to_value(t)?,
            Self::Number(n) => number_json(*n),
            Self::Missing => Value::Null,
        })
    }
}

/// 整数の値は整数の JSON にする（`u32` のフィールドに入るように）。
fn number_json(n: f64) -> Value {
    const EXACT_INT: f64 = 9.0e15;
    if n.fract() == 0.0 && n.abs() < EXACT_INT {
        Value::from(n as i64)
    } else {
        Value::from(n)
    }
}

/// 時刻の指定（`1.5`、`3/2`、`-0.4`）を読む。小数は 6 桁まで。
pub fn parse_time(raw: &str) -> Result<RationalTime> {
    let bad = || MmError::InvalidArgument(format!("時刻として読めない: {raw}"));
    let s = raw.trim();
    let (num, den) = if let Some((n, d)) = s.split_once('/') {
        (
            n.trim().parse::<i128>().map_err(|_| bad())?,
            d.trim().parse::<u32>().map_err(|_| bad())?,
        )
    } else {
        let (neg, body) = s.strip_prefix('-').map_or((false, s), |r| (true, r));
        let (int, frac) = body.split_once('.').unwrap_or((body, ""));
        let digits = |p: &str| p.chars().all(|c| c.is_ascii_digit());
        // 符号は先頭の 1 つだけ。整数の部分と小数の部分に符号が混じる `--1` や `1.-5` を拒む。
        if frac.len() > MAX_DECIMALS
            || (int.is_empty() && frac.is_empty())
            || !digits(int)
            || !digits(frac)
        {
            return Err(bad());
        }
        let magnitude: i128 = format!("{int}{frac}").parse().map_err(|_| bad())?;
        (
            if neg { -magnitude } else { magnitude },
            10u32.pow(frac.len() as u32),
        )
    };
    RationalTime::new(num, den)
}

/// 秒の浮動小数を千分の一秒に丸めた時刻にする（文字のスタイルの動きの長さ）。
pub(crate) fn millis(seconds: f64) -> Result<RationalTime> {
    const PER_SECOND: f64 = 1000.0;
    RationalTime::new((seconds * PER_SECOND).round() as i128, PER_SECOND as u32)
}

/// 引数と theme の値の表。`{{…}}` はこれを引く。
#[derive(Debug, Clone, Default)]
pub(crate) struct Env {
    values: BTreeMap<String, ArgValue>,
}

impl Env {
    /// マクロの引数を型どおりに読み、theme の値（`canvas.width` など）を足す。
    /// 知らない引数、足りない引数、読めない値は `InvalidArgument`。
    pub(crate) fn new(
        def: &MacroDef,
        theme: &Theme,
        raw: &BTreeMap<String, String>,
    ) -> Result<Self> {
        if let Some(unknown) = raw.keys().find(|k| !def.params.contains_key(*k)) {
            return Err(MmError::InvalidArgument(format!(
                "{} に引数 {unknown} は無い（あるのは {}）",
                def.name,
                def.params.keys().cloned().collect::<Vec<_>>().join(", ")
            )));
        }
        let mut values = theme_values(theme);
        for (name, param) in &def.params {
            let value = match raw.get(name).or(param.default.as_ref()) {
                Some(text) => ArgValue::parse(param.ty, text)
                    .map_err(|e| MmError::InvalidArgument(format!("引数 {name}: {e}")))?,
                None if param.optional => ArgValue::Missing,
                None => {
                    return Err(MmError::InvalidArgument(format!(
                        "{} の引数 {name} が要る",
                        def.name
                    )))
                }
            };
            values.insert(name.clone(), value);
        }
        Ok(Self { values })
    }

    /// 名前で引ける値か（検査が使う）。
    pub(crate) fn names(def: &MacroDef, theme: &Theme) -> Vec<String> {
        let mut names: Vec<String> = theme_values(theme).into_keys().collect();
        names.extend(def.params.keys().cloned());
        names
    }

    /// 文字列の `{{…}}` を文字として埋める。
    pub(crate) fn fill_text(&self, s: &str) -> Result<String> {
        let mut out = String::new();
        let mut rest = s;
        while let Some((before, expr, after)) = next_placeholder(rest)? {
            out.push_str(before);
            out.push_str(&self.eval(expr)?.to_text());
            rest = after;
        }
        out.push_str(rest);
        Ok(out)
    }

    /// JSON の中の文字列を全部埋める。全体が 1 つの `{{…}}` の文字列は型どおりの値に置き換える。
    pub(crate) fn fill_json(&self, value: &Value) -> Result<Value> {
        Ok(match value {
            Value::String(s) => match whole_placeholder(s) {
                Some(expr) => self.eval(expr)?.to_json()?,
                None => Value::String(self.fill_text(s)?),
            },
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|v| self.fill_json(v))
                    .collect::<Result<_>>()?,
            ),
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| Ok((k.clone(), self.fill_json(v)?)))
                    .collect::<Result<_>>()?,
            ),
            other => other.clone(),
        })
    }

    /// `名前` か、時刻の `a + b - 0.2` を計算する。
    fn eval(&self, expr: &str) -> Result<ArgValue> {
        let tokens: Vec<&str> = expr.split_whitespace().collect();
        if let [single] = tokens.as_slice() {
            return self.lookup(single);
        }
        let mut sum = self.time_operand(tokens.first().copied().unwrap_or(""))?;
        for pair in tokens[1..].chunks(2) {
            let [op, operand] = pair else {
                return Err(MmError::InvalidArgument(format!(
                    "式が途中で終わる: {expr}"
                )));
            };
            let rhs = self.time_operand(operand)?;
            sum = match *op {
                "+" => sum.checked_add(rhs)?,
                "-" => sum.checked_sub(rhs)?,
                _ => {
                    return Err(MmError::InvalidArgument(format!(
                        "式に使えるのは + と - だけ: {expr}"
                    )))
                }
            };
        }
        Ok(ArgValue::Time(sum))
    }

    fn lookup(&self, name: &str) -> Result<ArgValue> {
        self.values
            .get(name)
            .cloned()
            .ok_or_else(|| MmError::InvalidArgument(format!("{{{{{name}}}}} を引けない")))
    }

    fn time_operand(&self, token: &str) -> Result<RationalTime> {
        match self.values.get(token) {
            Some(ArgValue::Time(t)) => Ok(*t),
            Some(_) => Err(MmError::InvalidArgument(format!(
                "式で足し引きできるのは時刻だけ: {token}"
            ))),
            None => parse_time(token),
        }
    }
}

/// theme から引ける値。
fn theme_values(theme: &Theme) -> BTreeMap<String, ArgValue> {
    let mut values = BTreeMap::new();
    let canvases = [("canvas", Some(theme.canvas)), ("short", theme.short)];
    for (prefix, canvas) in canvases {
        let Some(c) = canvas else { continue };
        for (field, n) in [("width", c.width), ("height", c.height), ("fps", c.fps)] {
            values.insert(format!("{prefix}.{field}"), ArgValue::Number(f64::from(n)));
        }
    }
    values
}

/// 次の `{{…}}`。（前、式、後）を返す。閉じていなければ `InvalidArgument`。
fn next_placeholder(s: &str) -> Result<Option<(&str, &str, &str)>> {
    let Some(open) = s.find("{{") else {
        return Ok(None);
    };
    let inner = &s[open + 2..];
    let close = inner
        .find("}}")
        .ok_or_else(|| MmError::InvalidArgument(format!("{{{{ が閉じていない: {s}")))?;
    Ok(Some((
        &s[..open],
        inner[..close].trim(),
        &inner[close + 2..],
    )))
}

/// 文字列の全体が 1 つの `{{…}}` なら、その式。
fn whole_placeholder(s: &str) -> Option<&str> {
    let inner = s.strip_prefix("{{")?.strip_suffix("}}")?;
    (!inner.contains("{{") && !inner.contains("}}")).then(|| inner.trim())
}

/// 文字列の中の `{{…}}` が引く名前（演算子と数を除く）。検査が使う。
pub(crate) fn referenced_names(s: &str) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let mut rest = s;
    while let Some((_, expr, after)) = next_placeholder(rest)? {
        names.extend(
            expr.split_whitespace()
                .filter(|t| !matches!(*t, "+" | "-") && parse_time(t).is_err())
                .map(str::to_string),
        );
        rest = after;
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn def() -> MacroDef {
        serde_json::from_value(json!({
            "name": "m",
            "title": "M",
            "params": {
                "text": {"type": "text"},
                "at": {"type": "time"},
                "dur": {"type": "time", "default": "2"},
                "cut": {"type": "id", "optional": true}
            },
            "steps": []
        }))
        .unwrap()
    }

    fn theme() -> Theme {
        serde_json::from_value(json!({
            "colors": {},
            "motion": {"in_max": 0.8, "out_max": 0.3},
            "canvas": {"width": 1920, "height": 1080, "fps": 30}
        }))
        .unwrap()
    }

    fn env(pairs: &[(&str, &str)]) -> Result<Env> {
        let raw = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Env::new(&def(), &theme(), &raw)
    }

    #[test]
    fn 全体の置き換えは型どおりの値になる() {
        let e = env(&[("text", "やあ"), ("at", "1.5")]).unwrap();
        let filled = e
            .fill_json(
                &json!({"t": "{{at}}", "s": "{{text}}", "c": "{{cut}}", "w": "{{canvas.width}}"}),
            )
            .unwrap();
        assert_eq!(filled["t"], json!({"num": 3, "den": 2}));
        assert_eq!(filled["s"], json!("やあ"));
        assert_eq!(filled["c"], Value::Null);
        assert_eq!(filled["w"], json!(1920));
    }

    #[test]
    fn 一部の置き換えは文字になり_式で時刻を足し引きする() {
        let e = env(&[("text", "x"), ("at", "1")]).unwrap();
        assert_eq!(e.fill_text("id-{{text}}-{{cut}}").unwrap(), "id-x-");
        let end = e.fill_json(&json!("{{at + dur - 0.25}}")).unwrap();
        assert_eq!(end, json!({"num": 11, "den": 4}));
    }

    #[test]
    fn 足りない引数と知らない引数と読めない時刻を拒む() {
        assert!(env(&[("at", "1")]).is_err(), "text が無い");
        assert!(env(&[("text", "x"), ("at", "1"), ("zzz", "1")]).is_err());
        assert!(env(&[("text", "x"), ("at", "1.2.3")]).is_err());
        for bad in ["--1", "1.-5", "+1", "-.-5"] {
            assert!(parse_time(bad).is_err(), "{bad} を拒む");
        }
        assert_eq!(
            parse_time("-0.4").unwrap(),
            RationalTime::new(-2, 5).unwrap()
        );
        let e = env(&[("text", "x"), ("at", "1")]).unwrap();
        assert!(e.fill_text("{{nope}}").is_err());
        assert!(e.fill_text("{{at").is_err());
        assert!(e.fill_json(&json!("{{text + 1}}")).is_err(), "文は足せない");
    }

    #[test]
    fn 引く名前を集める() {
        let names = referenced_names("{{at + dur - 0.2}} と {{text}}").unwrap();
        assert_eq!(names, ["at", "dur", "text"]);
    }
}
