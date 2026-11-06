//! 時間の写像。コンポジションの時刻から素材の時刻へ（`start`、`in`、`speed`、タイムリマップ）、
//! その逆、入れ子をたどる合成。時刻は有理数のまま変換し、浮動小数を経ない。

use mm_doc::{Interpolation, Layer, TimeKeyframe, TimeRemap};
use mm_types::{MmError, RationalTime, Result};

/// コンポジションの時刻 `t` をレイヤーの素材の時刻に変換する。
///
/// タイムリマップがあればそれで、無ければ `in + (t - start) * speed`。
/// レイヤーの範囲の外の `t` も変換する（範囲の判定は呼ぶ側）。
pub fn source_time(layer: &Layer, t: RationalTime) -> Result<RationalTime> {
    let local = t.checked_sub(layer.start)?;
    match &layer.time_remap {
        Some(remap) => remap_forward(remap, local),
        None => local.scale(layer.speed)?.checked_add(layer.in_point),
    }
}

/// 素材の時刻 `s` をコンポジションの時刻に変換する（[`source_time`] の逆）。
///
/// タイムリマップは厳密に増える折れ線のときだけ逆に変換できる。そうでなければ `Unsupported`。
pub fn comp_time(layer: &Layer, s: RationalTime) -> Result<RationalTime> {
    let local = match &layer.time_remap {
        Some(remap) => remap_inverse(remap, s)?,
        None => s.checked_sub(layer.in_point)?.div_by(layer.speed)?,
    };
    local.checked_add(layer.start)
}

/// 入れ子の鎖をたどる。`layers[0]` が外側で、順に中へ変換する。
pub fn source_time_along(layers: &[&Layer], t: RationalTime) -> Result<RationalTime> {
    layers.iter().try_fold(t, |t, layer| source_time(layer, t))
}

/// 入れ子の鎖を内から外へ戻す（[`source_time_along`] の逆）。
pub fn comp_time_along(layers: &[&Layer], s: RationalTime) -> Result<RationalTime> {
    layers
        .iter()
        .rev()
        .try_fold(s, |s, layer| comp_time(layer, s))
}

/// タイムリマップの折れ線で、レイヤーの始まりからの時刻を素材の時刻に変換する。
pub fn remap_forward(remap: &TimeRemap, local: RationalTime) -> Result<RationalTime> {
    let keys = &remap.keyframes;
    let Some(first) = keys.first() else {
        return Err(MmError::InvalidArgument("タイムリマップが空".into()));
    };
    if local <= first.time {
        return Ok(first.source);
    }
    let i = keys.partition_point(|k| k.time <= local) - 1;
    let from = &keys[i];
    let Some(to) = keys.get(i + 1) else {
        return Ok(from.source);
    };
    match from.interpolation {
        Interpolation::Linear => lerp_time(from.time, from.source, to.time, to.source, local),
        _ => Ok(from.source),
    }
}

/// 厳密に増える折れ線の逆。範囲の外は端の時刻。
pub fn remap_inverse(remap: &TimeRemap, s: RationalTime) -> Result<RationalTime> {
    let keys = &remap.keyframes;
    if !is_strictly_increasing(keys) {
        return Err(MmError::Unsupported(
            "厳密に増えないタイムリマップは逆に変換できない".into(),
        ));
    }
    let Some(first) = keys.first() else {
        return Err(MmError::InvalidArgument("タイムリマップが空".into()));
    };
    if s <= first.source {
        return Ok(first.time);
    }
    let i = keys.partition_point(|k| k.source <= s) - 1;
    let from = &keys[i];
    let Some(to) = keys.get(i + 1) else {
        return Ok(from.time);
    };
    lerp_time(from.source, from.time, to.source, to.time, s)
}

fn is_strictly_increasing(keys: &[TimeKeyframe]) -> bool {
    keys.windows(2)
        .all(|w| w[0].source < w[1].source && w[0].interpolation == Interpolation::Linear)
}

/// `(x0, y0)` と `(x1, y1)` を結ぶ直線上の `x` での y を、有理数のまま求める。
/// `y0 + (x - x0) * (y1 - y0) / (x1 - x0)`。表せなければ `Err`。
fn lerp_time(
    x0: RationalTime,
    y0: RationalTime,
    x1: RationalTime,
    y1: RationalTime,
    x: RationalTime,
) -> Result<RationalTime> {
    let dx = x.checked_sub(x0)?;
    let dy = y1.checked_sub(y0)?;
    let span = x1.checked_sub(x0)?;
    // dx * dy / span。分子と分母の積は i128 であふれ得るので checked で計算し、先に約分する。
    let (n1, d1) = reduce(dx.num(), span.num());
    let (n2, d2) = reduce(dy.num(), i128::from(dx.den()) * i128::from(dy.den()));
    let num = n1
        .checked_mul(n2)
        .and_then(|n| n.checked_mul(i128::from(span.den())))
        .ok_or_else(overflow)?;
    let den = d1.checked_mul(d2).ok_or_else(overflow)?;
    let (num, den) = reduce(num, den);
    let den = u32::try_from(den).map_err(|_| overflow())?;
    y0.checked_add(RationalTime::new(num, den)?)
}

fn overflow() -> MmError {
    MmError::InvalidArgument("タイムリマップの計算が表せる範囲を超えた".into())
}

/// 符号を分子に寄せて約分する。
fn reduce(num: i128, den: i128) -> (i128, i128) {
    let sign = if den < 0 { -1 } else { 1 };
    let g = gcd(num.unsigned_abs(), den.unsigned_abs()).max(1) as i128;
    (sign * num / g, den.abs() / g)
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_doc::Node;
    use mm_types::Rational;
    use proptest::prelude::*;

    fn secs(s: i64) -> RationalTime {
        RationalTime::seconds(s)
    }

    fn t(num: i128, den: u32) -> RationalTime {
        RationalTime::new(num, den).unwrap()
    }

    fn layer(start: i64, in_s: i64, out_s: i64, speed: (u32, u32)) -> Layer {
        let mut l = Layer::new(
            "l",
            Node::new("solid", 1),
            secs(start),
            secs(in_s),
            secs(out_s),
        );
        l.speed = Rational::new(speed.0, speed.1).unwrap();
        l
    }

    fn remap(points: &[(i128, i128)]) -> TimeRemap {
        TimeRemap {
            keyframes: points
                .iter()
                .map(|&(time, source)| TimeKeyframe {
                    time: t(time, 1),
                    source: t(source, 1),
                    interpolation: Interpolation::Linear,
                })
                .collect(),
        }
    }

    #[test]
    fn 速さ_2_倍は素材を倍の速さで進む() {
        let l = layer(1, 10, 20, (2, 1));
        assert_eq!(source_time(&l, secs(1)).unwrap(), secs(10));
        assert_eq!(source_time(&l, secs(3)).unwrap(), secs(14));
        assert_eq!(comp_time(&l, secs(14)).unwrap(), secs(3));
    }

    #[test]
    fn タイムリマップの折れ線で変換する() {
        let mut l = layer(0, 0, 4, (1, 1));
        l.time_remap = Some(remap(&[(0, 0), (2, 1), (4, 5)]));
        assert_eq!(source_time(&l, secs(1)).unwrap(), t(1, 2));
        assert_eq!(source_time(&l, secs(3)).unwrap(), secs(3));
        assert_eq!(
            source_time(&l, secs(9)).unwrap(),
            secs(5),
            "最後より後は端の値"
        );
        assert_eq!(comp_time(&l, secs(3)).unwrap(), secs(3));
        assert_eq!(comp_time(&l, t(1, 2)).unwrap(), secs(1));
    }

    #[test]
    fn ホールドのタイムリマップは静止フレーム() {
        let mut l = layer(0, 0, 4, (1, 1));
        let mut r = remap(&[(0, 2), (4, 3)]);
        r.keyframes[0].interpolation = Interpolation::Hold;
        l.time_remap = Some(r);
        assert_eq!(source_time(&l, secs(3)).unwrap(), secs(2));
        assert!(matches!(
            comp_time(&l, secs(2)),
            Err(MmError::Unsupported(_))
        ));
    }

    #[test]
    fn 入れ子は外から中へ順に変換する() {
        let outer = layer(10, 0, 100, (1, 1));
        let inner = layer(1, 5, 100, (1, 2));
        let s = source_time_along(&[&outer, &inner], secs(13)).unwrap();
        // 外: 13 - 10 = 3。中: (3 - 1) / 2 + 5 = 6。
        assert_eq!(s, secs(6));
        assert_eq!(comp_time_along(&[&outer, &inner], s).unwrap(), secs(13));
    }

    fn arb_layer() -> impl Strategy<Value = Layer> {
        (-100i64..100, -100i64..100, 1i64..100, 1u32..8, 1u32..8)
            .prop_map(|(start, in_s, len, sn, sd)| layer(start, in_s, in_s + len, (sn, sd)))
    }

    fn arb_remap() -> impl Strategy<Value = TimeRemap> {
        // 時刻も素材も厳密に増える折れ線。
        prop::collection::vec((1i128..50, 1i128..50), 1..6).prop_map(|steps| {
            let mut points = vec![(0i128, 0i128)];
            for (dt, ds) in steps {
                let last = *points.last().unwrap();
                points.push((last.0 + dt, last.1 + ds));
            }
            remap(&points)
        })
    }

    proptest! {
        #[test]
        fn 速さの写像は単調で逆写像が戻る(l in arb_layer(), a in -1000i128..1000, b in -1000i128..1000) {
            let (ta, tb) = (t(a, 7), t(b, 7));
            let (sa, sb) = (source_time(&l, ta).unwrap(), source_time(&l, tb).unwrap());
            prop_assert_eq!(ta < tb, sa < sb);
            prop_assert_eq!(comp_time(&l, sa).unwrap(), ta);
        }

        #[test]
        fn タイムリマップの写像は単調で逆写像が戻る(r in arb_remap(), a in 0i128..3000, b in 0i128..3000) {
            let mut l = layer(0, 0, 1000, (1, 1));
            let last = r.keyframes.last().unwrap().time;
            l.time_remap = Some(r);
            let (ta, tb) = (t(a, 11), t(b, 11));
            let (sa, sb) = (source_time(&l, ta).unwrap(), source_time(&l, tb).unwrap());
            prop_assert!(ta >= tb || sa <= sb);
            if ta < last && tb < last {
                prop_assert_eq!(ta < tb, sa < sb);
                prop_assert_eq!(comp_time(&l, sa).unwrap(), ta);
            }
        }
    }
}
