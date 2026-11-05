//! 有理数の時刻。OTIO の `RationalTime` と同じ考え方で、量子化は葉で 1 回だけ行う。

use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Sub};

use serde::{Deserialize, Serialize};

use crate::error::{MmError, Result};

/// コマの番号を決めるときに足す丸めの余裕（1/2^20 コマ）。
/// 有理数の演算で 1 コマ境界ちょうどに乗った時刻が、前のコマに落ちないようにする。
pub const FRAME_SNAP_SHIFT: u32 = 20;

/// 正の有理数。fps と再生の速さに使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rational {
    num: u32,
    den: u32,
}

impl Rational {
    /// `num / den`。den が 0 なら `InvalidArgument`。
    pub fn new(num: u32, den: u32) -> Result<Self> {
        if den == 0 {
            return Err(MmError::InvalidArgument("Rational: den が 0".into()));
        }
        let g = gcd(num as u128, den as u128) as u32;
        Ok(Self {
            num: num / g,
            den: den / g,
        })
    }

    /// 整数の fps。
    pub const fn integer(n: u32) -> Self {
        Self { num: n, den: 1 }
    }

    /// 分子。
    pub const fn num(self) -> u32 {
        self.num
    }

    /// 分母。
    pub const fn den(self) -> u32 {
        self.den
    }

    /// 浮動小数への変換。表示にだけ使う。
    pub fn to_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

/// 有理数の時刻（秒）。負の値も持てる。常に既約で、分母は正。
///
/// 中の分子は i128 で、掛け合わせの途中であふれない。文書に書く形（[`RawTime`]）の分子は i64
/// （serde のタグ付き enum が i128 を運べないため）。den が 2^32 でも約 68 年を表せる。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(into = "RawTime", try_from = "RawTime")]
pub struct RationalTime {
    num: i128,
    den: u32,
}

/// 文書に書く形の時刻。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RawTime {
    pub num: i64,
    pub den: u32,
}

impl From<RationalTime> for RawTime {
    fn from(t: RationalTime) -> Self {
        let r = RationalTime::reduced(t.num, t.den);
        Self {
            num: i64::try_from(r.num).expect("時刻の分子が i64 を超えた"),
            den: r.den,
        }
    }
}

impl TryFrom<RawTime> for RationalTime {
    type Error = MmError;

    fn try_from(raw: RawTime) -> Result<Self> {
        Self::new(raw.num as i128, raw.den)
    }
}

impl RationalTime {
    /// 0 秒。
    pub const ZERO: Self = Self { num: 0, den: 1 };

    /// `num / den` 秒。den が 0 なら `InvalidArgument`。
    pub fn new(num: i128, den: u32) -> Result<Self> {
        if den == 0 {
            return Err(MmError::InvalidArgument("RationalTime: den が 0".into()));
        }
        Ok(Self::reduced(num, den))
    }

    /// 0 秒。serde の `default` に渡す関数の形。
    pub const fn zero() -> Self {
        Self::ZERO
    }

    /// 整数の秒。
    pub const fn seconds(s: i64) -> Self {
        Self {
            num: s as i128,
            den: 1,
        }
    }

    /// コマの番号から時刻へ（`frames / fps`）。
    pub fn from_frames(frames: i64, fps: Rational) -> Self {
        Self::reduced(frames as i128 * fps.den as i128, fps.num)
    }

    fn reduced(num: i128, den: u32) -> Self {
        let g = gcd(num.unsigned_abs(), den as u128);
        Self {
            num: num / g as i128,
            den: (den as u128 / g) as u32,
        }
    }

    /// 分子。
    pub const fn num(self) -> i128 {
        self.num
    }

    /// 分母。
    pub const fn den(self) -> u32 {
        self.den
    }

    /// 浮動小数の秒。表示とテストの許容差にだけ使う。
    pub fn to_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// 負の時刻か。
    pub const fn is_negative(self) -> bool {
        self.num < 0
    }

    /// 有理数倍（再生の速さ、fps）。
    pub fn scale(self, r: Rational) -> Self {
        Self::reduced(self.num * r.num as i128, self.den * r.den)
    }

    /// 有理数で割る。
    pub fn div_by(self, r: Rational) -> Self {
        Self::reduced(self.num * r.den as i128, self.den * r.num)
    }

    /// 素材のコマを選ぶ唯一の規則: `floor(t * fps + 1/2^20)`。
    /// 呼ぶ側は先に `t - offset` を素材の時間軸に写しておく。
    pub fn frame_index(self, fps: Rational) -> i64 {
        let shift = 1i128 << FRAME_SNAP_SHIFT;
        let den = self.den as i128 * fps.den as i128;
        let numer = self.num * fps.num as i128 * shift + den;
        div_floor(numer, den * shift) as i64
    }

    /// 左右を通分したときの分子の組。比較に使う。
    fn cross(self, other: Self) -> (i128, i128) {
        (self.num * other.den as i128, other.num * self.den as i128)
    }
}

fn div_floor(a: i128, b: i128) -> i128 {
    let q = a / b;
    if (a % b != 0) && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    if a == 0 {
        1
    } else {
        a
    }
}

impl PartialEq for RationalTime {
    fn eq(&self, other: &Self) -> bool {
        let (l, r) = self.cross(*other);
        l == r
    }
}

impl Eq for RationalTime {}

impl PartialOrd for RationalTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RationalTime {
    fn cmp(&self, other: &Self) -> Ordering {
        let (l, r) = self.cross(*other);
        l.cmp(&r)
    }
}

impl std::hash::Hash for RationalTime {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let r = Self::reduced(self.num, self.den);
        r.num.hash(state);
        r.den.hash(state);
    }
}

impl Add for RationalTime {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self::reduced(
            self.num * rhs.den as i128 + rhs.num * self.den as i128,
            self.den * rhs.den,
        )
    }
}

impl Sub for RationalTime {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self::reduced(
            self.num * rhs.den as i128 - rhs.num * self.den as i128,
            self.den * rhs.den,
        )
    }
}

impl fmt::Display for RationalTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}s", self.num, self.den)
    }
}

/// 半開区間 `[start, end)` の時刻の範囲。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: RationalTime,
    pub end: RationalTime,
}

impl TimeRange {
    /// `[start, end)`。end < start なら `InvalidArgument`。
    pub fn new(start: RationalTime, end: RationalTime) -> Result<Self> {
        if end < start {
            return Err(MmError::InvalidArgument(format!(
                "TimeRange: end {end} が start {start} より前"
            )));
        }
        Ok(Self { start, end })
    }

    /// 始まりと長さから。
    pub fn from_start_duration(start: RationalTime, duration: RationalTime) -> Result<Self> {
        Self::new(start, start + duration)
    }

    /// 長さ。
    pub fn duration(&self) -> RationalTime {
        self.end - self.start
    }

    /// 長さ 0 か。
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// `t` を含むか（end は含まない）。
    pub fn contains(&self, t: RationalTime) -> bool {
        self.start <= t && t < self.end
    }

    /// 重なりの範囲。重ならなければ `None`。
    pub fn intersect(&self, other: &Self) -> Option<Self> {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        (start < end).then_some(Self { start, end })
    }

    /// この範囲に含まれるコマの番号の列 `[first, last)`。
    pub fn frame_indices(&self, fps: Rational) -> std::ops::Range<i64> {
        self.start.frame_index(fps)..self.end.frame_index(fps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(num: i128, den: u32) -> RationalTime {
        RationalTime::new(num, den).unwrap()
    }

    #[test]
    fn 通分して比べる() {
        assert_eq!(t(1, 2), t(2, 4));
        assert!(t(1, 3) < t(1, 2));
        assert!(t(-1, 2) < RationalTime::ZERO);
    }

    #[test]
    fn 足し引きは既約になる() {
        let s = t(1, 6) + t(1, 3);
        assert_eq!((s.num(), s.den()), (1, 2));
        let d = t(1, 2) - t(1, 2);
        assert_eq!(d, RationalTime::ZERO);
    }

    #[test]
    fn コマ境界ちょうどは次のコマになる() {
        let fps = Rational::new(30000, 1001).unwrap();
        for i in 0..10_000 {
            let time = RationalTime::from_frames(i, fps);
            assert_eq!(time.frame_index(fps), i);
        }
    }

    #[test]
    fn コマ境界の少し手前は前のコマになる() {
        let fps = Rational::integer(60);
        let eps = t(1, 1_000_000);
        let time = RationalTime::from_frames(10, fps) - eps;
        assert_eq!(time.frame_index(fps), 9);
    }

    #[test]
    fn 負の時刻は床で切る() {
        let fps = Rational::integer(30);
        assert_eq!(t(-1, 60).frame_index(fps), -1);
    }

    #[test]
    fn 範囲の重なり() {
        let a = TimeRange::new(t(0, 1), t(2, 1)).unwrap();
        let b = TimeRange::new(t(1, 1), t(3, 1)).unwrap();
        let c = TimeRange::new(t(2, 1), t(3, 1)).unwrap();
        assert_eq!(a.intersect(&b).unwrap().duration(), t(1, 1));
        assert!(a.intersect(&c).is_none());
        assert!(!a.contains(a.end));
    }

    #[test]
    fn serde_の往復() {
        let v = t(1001, 30000);
        let s = serde_json::to_string(&v).unwrap();
        let back: RationalTime = serde_json::from_str(&s).unwrap();
        assert_eq!(v, back);
    }
}
