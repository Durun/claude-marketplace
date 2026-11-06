//! 時刻 t でのパラメータの値。キーフレームの補間とイージング。

use mm_doc::{CubicBezier, Ease, Interpolation, Keyframe, Node, Param};
use mm_types::{ParamValue, Params, RationalTime, Rgba};

/// ベジェの x を進み具合に合わせるときの許容差。
const BEZIER_EPSILON: f64 = 1e-7;
/// ニュートン法の反復の上限。収まらなければ二分法に落ちる。
const NEWTON_ITERATIONS: usize = 8;
/// 二分法の反復の上限（2^-40 まで詰める）。
const BISECTION_ITERATIONS: usize = 40;

/// 時刻 `t` でのパラメータの値。
///
/// キーフレームが無ければ `value`。最初のキーフレームより前は最初の値、最後以降は最後の値。
/// 区間の中は始点のキーフレームの補間で決まり、数の型（数、2・3 次元の数、色）だけ補間する。
pub fn eval_param(param: &Param, t: RationalTime) -> ParamValue {
    let keys = &param.keyframes;
    let Some(first) = keys.first() else {
        return param.value.clone();
    };
    if t <= first.time {
        return first.value.clone();
    }
    // 直前のキーフレーム（time <= t）の番号。
    let i = keys.partition_point(|k| k.time <= t) - 1;
    let from = &keys[i];
    let Some(to) = keys.get(i + 1) else {
        return from.value.clone();
    };
    let p = progress(from, to, t);
    let eased = match from.interpolation {
        Interpolation::Hold => return from.value.clone(),
        Interpolation::Linear => ease(from.ease, p),
        Interpolation::Bezier => from.bezier.unwrap_or_default().y_at(p),
    };
    lerp_value(&from.value, &to.value, eased)
}

/// ノードの全パラメータを時刻 `t` で評価する。
pub fn node_params_at(node: &Node, t: RationalTime) -> Params {
    let mut out = Params::new();
    for (k, p) in &node.params {
        out.set(k.clone(), eval_param(p, t));
    }
    out
}

/// `[from.time, to.time)` の中の `t` の進み具合（0..1）。時刻の有理数を浮動小数に落とすのはここだけ。
fn progress(from: &Keyframe, to: &Keyframe, t: RationalTime) -> f64 {
    let span = to.time.to_f64() - from.time.to_f64();
    if span <= 0.0 {
        return 1.0;
    }
    ((t.to_f64() - from.time.to_f64()) / span).clamp(0.0, 1.0)
}

/// イージング（2 次）。`p ∈ [0,1]` を `[0,1]` の中で単調に曲げる。
pub fn ease(kind: Ease, p: f64) -> f64 {
    match kind {
        Ease::None => p,
        Ease::In => p * p,
        Ease::Out => 1.0 - (1.0 - p) * (1.0 - p),
        Ease::InOut if p < 0.5 => 2.0 * p * p,
        Ease::InOut => 1.0 - 2.0 * (1.0 - p) * (1.0 - p),
    }
}

/// `CubicBezier` の評価。CSS の `cubic-bezier` と同じ。
pub trait BezierCurve {
    /// 進み具合 `x` に対応する曲線の y。
    fn y_at(&self, x: f64) -> f64;
}

impl BezierCurve for CubicBezier {
    fn y_at(&self, x: f64) -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        let s = solve_bezier_x(self, x);
        bezier_1d(self.y1, self.y2, s)
    }
}

/// 端が 0 と 1、制御点が `c1`、`c2` の 1 次元の 3 次ベジェ。
fn bezier_1d(c1: f64, c2: f64, s: f64) -> f64 {
    let u = 1.0 - s;
    3.0 * u * u * s * c1 + 3.0 * u * s * s * c2 + s * s * s
}

fn bezier_1d_derivative(c1: f64, c2: f64, s: f64) -> f64 {
    let u = 1.0 - s;
    3.0 * u * u * c1 + 6.0 * u * s * (c2 - c1) + 3.0 * s * s * (1.0 - c2)
}

/// `x(s) = x` を満たす `s`。ニュートン法で寄せ、収まらなければ二分法。
fn solve_bezier_x(curve: &CubicBezier, x: f64) -> f64 {
    let mut s = x;
    for _ in 0..NEWTON_ITERATIONS {
        let dx = bezier_1d(curve.x1, curve.x2, s) - x;
        if dx.abs() < BEZIER_EPSILON {
            return s;
        }
        let d = bezier_1d_derivative(curve.x1, curve.x2, s);
        if d.abs() < BEZIER_EPSILON {
            break;
        }
        s -= dx / d;
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..BISECTION_ITERATIONS {
        s = (lo + hi) / 2.0;
        if bezier_1d(curve.x1, curve.x2, s) < x {
            lo = s;
        } else {
            hi = s;
        }
    }
    s
}

/// 数の型の線形補間。型が違うか補間できない型なら `a` を保つ。
pub fn lerp_value(a: &ParamValue, b: &ParamValue, p: f64) -> ParamValue {
    match (a, b) {
        (ParamValue::Number(x), ParamValue::Number(y)) => ParamValue::Number(lerp(*x, *y, p)),
        (ParamValue::Vec2(x), ParamValue::Vec2(y)) => {
            ParamValue::Vec2([lerp(x[0], y[0], p), lerp(x[1], y[1], p)])
        }
        (ParamValue::Vec3(x), ParamValue::Vec3(y)) => ParamValue::Vec3([
            lerp(x[0], y[0], p),
            lerp(x[1], y[1], p),
            lerp(x[2], y[2], p),
        ]),
        (ParamValue::Color(x), ParamValue::Color(y)) => ParamValue::Color(lerp_color(*x, *y, p)),
        _ => a.clone(),
    }
}

fn lerp(a: f64, b: f64, p: f64) -> f64 {
    a + (b - a) * p
}

/// 色の補間。線形の premultiplied の値をそのまま成分ごとに混ぜる。
fn lerp_color(a: Rgba, b: Rgba, p: f64) -> Rgba {
    let k = |x: f32, y: f32| lerp(f64::from(x), f64::from(y), p) as f32;
    Rgba::new(k(a.r, b.r), k(a.g, b.g), k(a.b, b.b), k(a.a, b.a))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn key(t: RationalTime, v: f64, i: Interpolation) -> Keyframe {
        Keyframe::new(t, ParamValue::Number(v), i)
    }

    fn secs(s: i64) -> RationalTime {
        RationalTime::seconds(s)
    }

    fn param(keys: Vec<Keyframe>) -> Param {
        Param {
            value: ParamValue::Number(-99.0),
            keyframes: keys,
        }
    }

    #[test]
    fn ホールドは直前のキーフレーム_端は端の値() {
        let p = param(vec![
            key(secs(1), 1.0, Interpolation::Hold),
            key(secs(2), 2.0, Interpolation::Hold),
        ]);
        assert_eq!(eval_param(&p, RationalTime::ZERO).as_number(), Some(1.0));
        assert_eq!(
            eval_param(&p, RationalTime::new(3, 2).unwrap()).as_number(),
            Some(1.0)
        );
        assert_eq!(eval_param(&p, secs(5)).as_number(), Some(2.0));
    }

    #[test]
    fn キーフレームが無ければ固定の値() {
        let p = Param::constant(ParamValue::Number(3.0));
        assert_eq!(eval_param(&p, secs(7)).as_number(), Some(3.0));
    }

    #[test]
    fn 直線は中点で平均() {
        let p = param(vec![
            key(secs(0), 0.0, Interpolation::Linear),
            key(secs(2), 10.0, Interpolation::Linear),
        ]);
        assert_eq!(eval_param(&p, secs(1)).as_number(), Some(5.0));
    }

    #[test]
    fn 色と_vec2_も補間する() {
        let p = Param {
            value: ParamValue::Vec2([0.0, 0.0]),
            keyframes: vec![
                Keyframe::new(secs(0), ParamValue::Vec2([0.0, 0.0]), Interpolation::Linear),
                Keyframe::new(secs(2), ParamValue::Vec2([2.0, 4.0]), Interpolation::Linear),
            ],
        };
        assert_eq!(eval_param(&p, secs(1)).as_vec2(), Some([1.0, 2.0]));
        let c = lerp_value(
            &ParamValue::Color(Rgba::TRANSPARENT),
            &ParamValue::Color(Rgba::WHITE),
            0.5,
        );
        assert_eq!(c.as_color(), Some(Rgba::new(0.5, 0.5, 0.5, 0.5)));
    }

    #[test]
    fn 補間できない型はホールド() {
        let p = Param {
            value: ParamValue::Text(String::new()),
            keyframes: vec![
                Keyframe::new(secs(0), ParamValue::Text("a".into()), Interpolation::Linear),
                Keyframe::new(secs(2), ParamValue::Text("b".into()), Interpolation::Linear),
            ],
        };
        assert_eq!(eval_param(&p, secs(1)).as_text(), Some("a"));
    }

    #[test]
    fn ベジェの既定は_ease_in_out_で対称() {
        let c = CubicBezier::EASE_IN_OUT;
        assert!((c.y_at(0.5) - 0.5).abs() < 1e-6);
        assert!(c.y_at(0.25) < 0.25);
        assert!(c.y_at(0.75) > 0.75);
        assert!((c.y_at(0.3) + c.y_at(0.7) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn 直線のベジェは恒等() {
        let c = CubicBezier {
            x1: 0.25,
            y1: 0.25,
            x2: 0.75,
            y2: 0.75,
        };
        for i in 0..=10 {
            let x = f64::from(i) / 10.0;
            assert!((c.y_at(x) - x).abs() < 1e-6, "x={x}");
        }
    }

    fn arb_ease() -> impl Strategy<Value = Ease> {
        prop_oneof![
            Just(Ease::None),
            Just(Ease::In),
            Just(Ease::Out),
            Just(Ease::InOut)
        ]
    }

    fn arb_unit_bezier() -> impl Strategy<Value = CubicBezier> {
        (0.0..=1.0, 0.0..=1.0, 0.0..=1.0, 0.0..=1.0).prop_map(|(x1, y1, x2, y2)| CubicBezier {
            x1,
            y1,
            x2,
            y2,
        })
    }

    /// 2 つのキーフレーム（時刻 0 と `span`、値 `a` と `b`）と、その間の時刻。
    fn arb_segment() -> impl Strategy<Value = (f64, f64, i64, i64)> {
        (-1000.0..1000.0, -1000.0..1000.0, 1i64..1000, 0i64..=1000)
    }

    proptest! {
        #[test]
        fn 直線とイージングは端の値の間に収まる((a, b, span, k) in arb_segment(), e in arb_ease()) {
            let mut from = key(RationalTime::ZERO, a, Interpolation::Linear);
            from.ease = e;
            let p = param(vec![from, key(secs(span), b, Interpolation::Linear)]);
            let t = RationalTime::new(i128::from(span) * i128::from(k), 1000).unwrap();
            let v = eval_param(&p, t).as_number().unwrap();
            prop_assert!(v >= a.min(b) - 1e-9 && v <= a.max(b) + 1e-9, "v={v} a={a} b={b}");
        }

        #[test]
        fn 単位正方形のベジェは端の値の間に収まる((a, b, span, k) in arb_segment(), c in arb_unit_bezier()) {
            let mut from = key(RationalTime::ZERO, a, Interpolation::Bezier);
            from.bezier = Some(c);
            let p = param(vec![from, key(secs(span), b, Interpolation::Linear)]);
            let t = RationalTime::new(i128::from(span) * i128::from(k), 1000).unwrap();
            let v = eval_param(&p, t).as_number().unwrap();
            prop_assert!(v >= a.min(b) - 1e-6 && v <= a.max(b) + 1e-6, "v={v} a={a} b={b}");
        }

        #[test]
        fn キーフレームの時刻ではちょうどその値(values in prop::collection::vec(-1000.0..1000.0f64, 1..6), e in arb_ease()) {
            let keys: Vec<Keyframe> = values
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let mut k = key(secs(i as i64), *v, Interpolation::Linear);
                    k.ease = e;
                    k
                })
                .collect();
            let p = param(keys);
            for (i, v) in values.iter().enumerate() {
                prop_assert_eq!(eval_param(&p, secs(i as i64)).as_number(), Some(*v));
            }
        }

        #[test]
        fn イージングは単調で端が_0_と_1(e in arb_ease(), x in 0.0..=1.0f64, y in 0.0..=1.0f64) {
            prop_assert_eq!(ease(e, 0.0), 0.0);
            prop_assert_eq!(ease(e, 1.0), 1.0);
            let (lo, hi) = (x.min(y), x.max(y));
            prop_assert!(ease(e, lo) <= ease(e, hi) + 1e-12);
        }

        #[test]
        fn ベジェは_x_の方向に単調(c in arb_unit_bezier(), x in 0.0..=1.0f64, y in 0.0..=1.0f64) {
            // y1 <= y2 の曲線は単調。そうでない曲線は値の範囲だけ保つ。
            let (lo, hi) = (x.min(y), x.max(y));
            if c.y1 <= c.y2 {
                prop_assert!(c.y_at(lo) <= c.y_at(hi) + 1e-6);
            }
            prop_assert!((-1e-6..=1.0 + 1e-6).contains(&c.y_at(x)));
        }
    }
}
