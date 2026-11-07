//! geneva のタイムラインの値の読み方（時刻、長さ、点、時間で変わる値）。書式は `geneva guide timeline`。

use mm_types::{MmError, Rational, RationalTime, Result};
use serde_json::Value;

/// 小数の秒を有理数にするときの桁の上限。分母が `u32` に収まる 10^9 まで。
const MAX_DECIMALS: u32 = 9;
/// `"250ms"` の換算。
const MS_PER_SECOND: u32 = 1000;

/// geneva の時刻（`"112f"`、`"0.5s"`、`"250ms"`、数は秒）を有理数にする。`fps` は `"Nf"` の換算に使う。
pub fn time(value: &Value, fps: Rational) -> Result<RationalTime> {
    match value {
        Value::Number(n) => decimal(&n.to_string()),
        Value::String(s) => time_text(s.trim(), fps),
        other => Err(bad("時刻", other)),
    }
}

fn time_text(s: &str, fps: Rational) -> Result<RationalTime> {
    if let Some(frames) = s.strip_suffix('f') {
        let n: i128 = frames
            .parse()
            .map_err(|_| MmError::InvalidArgument(format!("フレーム数でない: {s}")))?;
        return RationalTime::new(n * i128::from(fps.den()), fps.num());
    }
    if let Some(ms) = s.strip_suffix("ms") {
        return decimal(ms)?.scale(Rational::new(1, MS_PER_SECOND)?);
    }
    decimal(s.strip_suffix('s').unwrap_or(s))
}

/// 10 進の小数の文字列を、丸めずに有理数にする（`"258.56"` は 25856/100）。桁が多すぎれば丸める。
pub fn decimal(s: &str) -> Result<RationalTime> {
    let s = s.trim();
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    let frac = &frac[..frac.len().min(MAX_DECIMALS as usize)];
    let digits = format!("{int}{frac}");
    let num: i128 = digits
        .parse()
        .map_err(|_| MmError::InvalidArgument(format!("数でない: {s}")))?;
    RationalTime::new(num, 10u32.pow(frac.len() as u32))
}

/// 長さ（画素の数、`"12px"`、`"50%"`）。割合は `reference` に対する。
pub fn length(value: &Value, reference: f64) -> Result<f64> {
    match value {
        Value::Number(n) => n.as_f64().ok_or_else(|| bad("長さ", value)),
        Value::String(s) => length_text(s.trim(), reference),
        other => Err(bad("長さ", other)),
    }
}

fn length_text(s: &str, reference: f64) -> Result<f64> {
    let parse = |t: &str| {
        t.trim()
            .parse::<f64>()
            .map_err(|_| MmError::InvalidArgument(format!("長さでない: {s}")))
    };
    if let Some(p) = s.strip_suffix('%') {
        return Ok(parse(p)? / 100.0 * reference);
    }
    match s {
        "center" => Ok(reference / 2.0),
        "left" | "top" => Ok(0.0),
        "right" | "bottom" => Ok(reference),
        _ => parse(s.strip_suffix("px").unwrap_or(s)),
    }
}

/// 点（`{x, y}`、`[x, y]`、`"x y"`、`"center"`）。割合は `reference`（幅、高さ）に対する。
pub fn point(value: &Value, reference: [f64; 2]) -> Result<[f64; 2]> {
    let pair = |a: &Value, b: &Value| Ok([length(a, reference[0])?, length(b, reference[1])?]);
    match value {
        Value::Object(o) => {
            let zero = Value::from(0);
            pair(o.get("x").unwrap_or(&zero), o.get("y").unwrap_or(&zero))
        }
        Value::Array(a) if a.len() == 2 => pair(&a[0], &a[1]),
        Value::String(s) => {
            let parts: Vec<&str> = s.split_whitespace().collect();
            match parts.as_slice() {
                [one] => pair(&Value::from(*one), &Value::from(*one)),
                [x, y] => pair(&Value::from(*x), &Value::from(*y)),
                _ => Err(bad("点", value)),
            }
        }
        other => Err(bad("点", other)),
    }
}

/// 拡大（数か `{x, y}`）を軸ごとの倍率にする。
pub fn scale(value: &Value) -> Result<[f64; 2]> {
    match value {
        Value::Number(n) => {
            let s = n.as_f64().ok_or_else(|| bad("拡大", value))?;
            Ok([s, s])
        }
        Value::Object(o) => {
            let axis = |k: &str| o.get(k).and_then(Value::as_f64).unwrap_or(1.0);
            Ok([axis("x"), axis("y")])
        }
        other => Err(bad("拡大", other)),
    }
}

/// 時間で変わる値の 1 点。時刻はクリップの始まりから。
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub time: RationalTime,
    pub value: [f64; 2],
    /// `linear` 以外のイージング（直線で近似した）。
    pub eased: bool,
}

/// 時間で変わる値。キーが 1 つなら固定の値。
#[derive(Debug, Clone, PartialEq)]
pub struct Anim {
    pub keys: Vec<Key>,
}

impl Anim {
    /// 固定の値。
    pub fn constant(value: [f64; 2]) -> Self {
        Self {
            keys: vec![Key {
                time: RationalTime::ZERO,
                value,
                eased: false,
            }],
        }
    }

    /// キーフレームを持つか。
    pub fn is_animated(&self) -> bool {
        self.keys.len() > 1
    }

    /// 直線で近似したイージングがあるか。
    pub fn has_easing(&self) -> bool {
        self.keys.iter().any(|k| k.eased)
    }

    /// 時刻 `t`（クリップの始まりから）の値。前後は端の値、間は直線で補間する。
    pub fn at(&self, t: RationalTime) -> [f64; 2] {
        let i = self.keys.partition_point(|k| k.time <= t);
        if i == 0 {
            return self.keys[0].value;
        }
        let Some(next) = self.keys.get(i) else {
            return self.keys[i - 1].value;
        };
        let prev = &self.keys[i - 1];
        let span = next.time.to_f64() - prev.time.to_f64();
        let p = (t.to_f64() - prev.time.to_f64()) / span;
        [0, 1].map(|c| prev.value[c] + (next.value[c] - prev.value[c]) * p)
    }
}

/// 時間で変わる値を読む。`one` が固定の値 1 つを 2 成分にする（数なら両方に同じ値）。
pub fn anim(
    value: &Value,
    fps: Rational,
    one: &dyn Fn(&Value) -> Result<[f64; 2]>,
) -> Result<Anim> {
    let Some(list) = value.get("keyframes").and_then(Value::as_array) else {
        return Ok(Anim::constant(one(value)?));
    };
    let mut keys = Vec::with_capacity(list.len());
    for entry in list {
        let (t, v, ease) = match entry {
            Value::Array(a) if a.len() >= 2 => (&a[0], &a[1], a.get(2)),
            Value::Object(o) => (
                o.get("t").ok_or_else(|| bad("キーフレーム", entry))?,
                o.get("v").ok_or_else(|| bad("キーフレーム", entry))?,
                o.get("ease"),
            ),
            other => return Err(bad("キーフレーム", other)),
        };
        keys.push(Key {
            time: time(t, fps)?,
            value: one(v)?,
            eased: ease.is_some_and(|e| e.as_str() != Some("linear")),
        });
    }
    if keys.is_empty() {
        return Err(bad("キーフレーム", value));
    }
    Ok(Anim { keys })
}

/// 数を 2 成分にする（両方に同じ値）。
pub fn number(value: &Value) -> Result<[f64; 2]> {
    let n = value.as_f64().ok_or_else(|| bad("数", value))?;
    Ok([n, n])
}

fn bad(what: &str, value: &Value) -> MmError {
    MmError::InvalidArgument(format!("{what}として読めない: {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fps30() -> Rational {
        Rational::integer(30)
    }

    #[test]
    fn フレームと秒と小数を丸めずに読む() {
        let f = time(&json!("112f"), fps30()).unwrap();
        assert_eq!(f, RationalTime::new(112, 30).unwrap());
        let s = time(&json!("0.0333s"), fps30()).unwrap();
        assert_eq!(s, RationalTime::new(333, 10_000).unwrap());
        let n = time(&json!(258.56), fps30()).unwrap();
        assert_eq!(n, RationalTime::new(25_856, 100).unwrap());
        let ms = time(&json!("250ms"), fps30()).unwrap();
        assert_eq!(ms, RationalTime::new(1, 4).unwrap());
        let fine = time(&json!("1.25ms"), fps30()).unwrap();
        assert_eq!(fine, RationalTime::new(1, 800).unwrap());
        // 分母に収まらない桁は、丸めた別の値にせず誤りにする
        assert!(time(&json!("0.123456789ms"), fps30()).is_err());
    }

    #[test]
    fn 点の書き方をどれも読む() {
        let box_size = [200.0, 100.0];
        assert_eq!(point(&json!("0% 0%"), box_size).unwrap(), [0.0, 0.0]);
        assert_eq!(point(&json!("50% 50%"), box_size).unwrap(), [100.0, 50.0]);
        assert_eq!(point(&json!("0 0"), box_size).unwrap(), [0.0, 0.0]);
        assert_eq!(point(&json!([3, 4]), box_size).unwrap(), [3.0, 4.0]);
        assert_eq!(
            point(&json!({"x": 1513, "y": "10px"}), box_size).unwrap(),
            [1513.0, 10.0]
        );
        assert_eq!(point(&json!("center"), box_size).unwrap(), [100.0, 50.0]);
    }

    #[test]
    fn キーフレームの間は直線で補間する() {
        let v = json!({"keyframes": [["0s", [0.0, 10.0]], ["1s", [10.0, 20.0]]]});
        let a = anim(&v, fps30(), &|x| point(x, [1.0, 1.0])).unwrap();
        let half = RationalTime::new(1, 2).unwrap();
        assert_eq!(a.at(half), [5.0, 15.0]);
        assert_eq!(a.at(RationalTime::new(2, 1).unwrap()), [10.0, 20.0]);
        assert!(!a.has_easing());
    }

    #[test]
    fn 直線でないイージングを記録する() {
        let v = json!({"keyframes": [{"t": 0, "v": 0}, {"t": "0.5s", "v": 1, "ease": "ease-out"}]});
        let a = anim(&v, fps30(), &number).unwrap();
        assert!(a.has_easing());
    }
}
