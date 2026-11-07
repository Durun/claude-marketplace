//! OTIO の `RationalTime`（`value / rate`、どちらも浮動小数）と `RationalTime`（有理数の秒）の変換。
//!
//! 書くときは、素材の fps の整数フレームに乗る時刻を `value` = フレーム数、`rate` = fps にする。
//! 乗らない時刻は `value` = 分子、`rate` = 分母にして厳密さを保つ。
//! 読むときは浮動小数を連分数で有理数に戻す（29.97 ではなく 30000/1001 になる）。

use serde_json::{json, Value};

use mm_types::{MmError, Rational, RationalTime, Result};

/// 浮動小数を有理数に直す分母の上限。fps の 30000/1001 や 24000/1001 を十分に含む。
const MAX_DEN: i128 = 1_000_000;
/// 有理数に直したときの許容する相対誤差。`f64` の 10 進表記の丸め（29.97002997002997 など）を許す。
const RELATIVE_TOLERANCE: f64 = 1e-9;
/// 連分数を打ち切る項の数。
const MAX_TERMS: usize = 48;
/// `f64` が整数を厳密に持てる上限。
const MAX_EXACT: i128 = 1 << 53;
/// メタデータの厳密な値と OTIO の値が同じとみなす差（秒）。
const NEAR_SECONDS: f64 = 1e-6;
/// メタデータの厳密な速さと OTIO の速さが同じとみなす差（倍率）。
const NEAR_SPEED: f64 = 1e-6;

fn invalid(msg: String) -> MmError {
    MmError::InvalidArgument(msg)
}

fn gcd(mut a: i128, mut b: i128) -> i128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.abs().max(1)
}

/// 有限の `x` を、分母が [`MAX_DEN`] 以下で許容誤差に収まる有理数 `(分子, 分母)` に直す。
pub(super) fn ratio_from_f64(x: f64) -> Option<(i128, i128)> {
    // 整数を厳密に持てない大きさは、分子どうしの掛け算があふれるので受けない。
    if !x.is_finite() || x.abs() >= MAX_EXACT as f64 {
        return None;
    }
    if x.fract() == 0.0 {
        return Some((x as i128, 1));
    }
    let tol = RELATIVE_TOLERANCE * x.abs().max(1.0);
    let (mut h0, mut h1, mut k0, mut k1) = (0i128, 1i128, 1i128, 0i128);
    let mut rest = x.abs();
    for _ in 0..MAX_TERMS {
        let a = rest.floor() as i128;
        let h2 = a.checked_mul(h1)?.checked_add(h0)?;
        let k2 = a.checked_mul(k1)?.checked_add(k0)?;
        if k2 > MAX_DEN {
            return None;
        }
        (h0, h1, k0, k1) = (h1, h2, k1, k2);
        if (x.abs() - h1 as f64 / k1 as f64).abs() <= tol {
            let sign = if x < 0.0 { -1 } else { 1 };
            return Some((sign * h1, k1));
        }
        let frac = rest - rest.floor();
        if frac == 0.0 {
            return None;
        }
        rest = 1.0 / frac;
    }
    None
}

/// 正の速さ・fps を `Rational` に直す。
pub(super) fn rational_from_f64(x: f64) -> Option<Rational> {
    let (n, d) = ratio_from_f64(x)?;
    Rational::new(u32::try_from(n).ok()?, u32::try_from(d).ok()?).ok()
}

/// 有理数の秒を OTIO の `RationalTime.1` にする。`rate` の整数フレームに乗らなければ `value` = 分子、`rate` = 分母。
pub(super) fn time_to_json(t: RationalTime, rate: Rational) -> Result<Value> {
    let numer = t.num() * rate.num() as i128;
    let denom = t.den() as i128 * rate.den() as i128;
    let (value, rate_f) = if numer % denom == 0 && (numer / denom).abs() < MAX_EXACT {
        ((numer / denom) as f64, rate.to_f64())
    } else if t.num().abs() < MAX_EXACT {
        (t.num() as f64, t.den() as f64)
    } else {
        return Err(invalid(format!("OTIO に書けない大きさの時刻: {t}")));
    };
    Ok(json!({"OTIO_SCHEMA": "RationalTime.1", "rate": rate_f, "value": value}))
}

/// OTIO の `RationalTime` を有理数の秒に戻す。
pub(super) fn time_from_json(v: &Value) -> Result<RationalTime> {
    let field = |name: &str| {
        v.get(name)
            .and_then(Value::as_f64)
            .ok_or_else(|| invalid(format!("RationalTime に数の {name} が無い: {v}")))
    };
    let (value, rate) = (field("value")?, field("rate")?);
    let (rn, rd) = ratio_from_f64(rate)
        .filter(|(n, _)| *n > 0)
        .ok_or_else(|| invalid(format!("RationalTime の rate を有理数にできない: {rate}")))?;
    let (vn, vd) = ratio_from_f64(value)
        .ok_or_else(|| invalid(format!("RationalTime の value を有理数にできない: {value}")))?;
    let (num, den) = vn
        .checked_mul(rd)
        .zip(vd.checked_mul(rn))
        .ok_or_else(|| invalid(format!("RationalTime が大きすぎる: {value}/{rate}")))?;
    let g = gcd(num, den);
    let den = u32::try_from(den / g)
        .map_err(|_| invalid(format!("RationalTime の分母が大きすぎる: {value}/{rate}")))?;
    RationalTime::new(num / g, den)
}

/// `TimeRange.1`。
pub(super) fn range_to_json(
    start: RationalTime,
    duration: RationalTime,
    rate: Rational,
) -> Result<Value> {
    Ok(json!({
        "OTIO_SCHEMA": "TimeRange.1",
        "start_time": time_to_json(start, rate)?,
        "duration": time_to_json(duration, rate)?,
    }))
}

/// `TimeRange` を `(開始, 長さ)` に戻す。null や無いときは `None`。
pub(super) fn range_from_json(v: Option<&Value>) -> Result<Option<(RationalTime, RationalTime)>> {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let part = |name: &str| {
        v.get(name)
            .ok_or_else(|| invalid(format!("TimeRange に {name} が無い")))
            .and_then(time_from_json)
    };
    Ok(Some((part("start_time")?, part("duration")?)))
}

/// 2 つの時刻が [`NEAR_SECONDS`] の範囲で等しいか。メタデータの厳密な値を使ってよいかの判定に使う。
pub(super) fn near(a: RationalTime, b: RationalTime) -> bool {
    (a.to_f64() - b.to_f64()).abs() <= NEAR_SECONDS
}

/// 2 つの速さが [`NEAR_SPEED`] の範囲で等しいか。
pub(super) fn near_speed(a: Rational, b: Rational) -> bool {
    (a.to_f64() - b.to_f64()).abs() <= NEAR_SPEED
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(n: i128, d: u32) -> RationalTime {
        RationalTime::new(n, d).unwrap()
    }

    #[test]
    fn ntsc_のレートは分数に戻る() {
        assert_eq!(ratio_from_f64(29.97002997002997), Some((30000, 1001)));
        assert_eq!(ratio_from_f64(23.976023976023978), Some((24000, 1001)));
        assert_eq!(ratio_from_f64(59.94005994005994), Some((60000, 1001)));
        assert_eq!(ratio_from_f64(24.0), Some((24, 1)));
        assert_eq!(ratio_from_f64(-10.5), Some((-21, 2)));
    }

    #[test]
    fn フレームに乗る時刻は_fps_で書き乗らない時刻は分子と分母で書く() {
        let fps = Rational::new(30000, 1001).unwrap();
        let on = time_to_json(t(1001, 3000), fps).unwrap();
        assert_eq!(on["value"], 10.0);
        let off = time_to_json(t(1, 7), fps).unwrap();
        assert_eq!(
            (off["value"].as_f64(), off["rate"].as_f64()),
            (Some(1.0), Some(7.0))
        );
    }

    #[test]
    fn 書いた時刻はそのまま読める() {
        let fps = Rational::new(30000, 1001).unwrap();
        for time in [
            t(0, 1),
            t(1001, 3000),
            t(1, 7),
            t(-5, 3),
            t(123456789, 48000),
        ] {
            let back = time_from_json(&time_to_json(time, fps).unwrap()).unwrap();
            assert_eq!(back, time);
        }
    }

    #[test]
    fn 正でない_rate_や非有限は拒否する() {
        let v = json!({"rate": 0.0, "value": 1.0});
        assert!(time_from_json(&v).is_err());
        assert_eq!(ratio_from_f64(f64::NAN), None);
    }

    #[test]
    fn 整数を厳密に持てない大きさの_value_と_rate_はあふれずに拒否する() {
        for (value, rate) in [(1e38, 2.5), (0.5, 1e38), (2.5e37, 24.0), (9.1e15, 1e6)] {
            let v = json!({"rate": rate, "value": value});
            assert!(time_from_json(&v).is_err(), "{value}/{rate}");
        }
    }
}
