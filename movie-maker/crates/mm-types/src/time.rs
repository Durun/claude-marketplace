//! 有理数の時刻。OTIO の `RationalTime` と同じ考え方で、量子化は葉で 1 回だけ行う。
//!
//! 時刻の演算（足す、引く、倍する）は結果が表せなければ `Err` を返す。90kHz の PTS と 48kHz の
//! 音のように分母が違う時刻は、最小公倍数の上で計算するのであふれない。

use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{MmError, Result};

/// コマの番号を決めるときに足す丸めの余裕（1/2^20 コマ）。
/// 有理数の演算で 1 コマ境界ちょうどに乗った時刻が、前のコマに落ちないようにする。
pub const FRAME_SNAP_SHIFT: u32 = 20;

/// 時刻の分子の絶対値の上限。文書に書く形（i64）に必ず収め、通分の掛け算が i128 であふれないようにする。
const MAX_NUM: i128 = i64::MAX as i128;

/// 正の有理数。fps と再生の速さに使う。常に既約で、分子も分母も 1 以上。
/// 読み込みも [`Rational::new`] を通るので、`{"num":30,"den":0}` や `num:0` は読めない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RawRational")]
pub struct Rational {
    num: u32,
    den: u32,
}

/// 文書に書く形の有理数。
#[derive(Deserialize)]
struct RawRational {
    num: u32,
    den: u32,
}

impl TryFrom<RawRational> for Rational {
    type Error = MmError;

    fn try_from(raw: RawRational) -> Result<Self> {
        Self::new(raw.num, raw.den)
    }
}

impl Rational {
    /// `num / den`（約分する）。num か den が 0 なら `InvalidArgument`。
    pub fn new(num: u32, den: u32) -> Result<Self> {
        if num == 0 || den == 0 {
            return Err(MmError::InvalidArgument(format!(
                "Rational: 正の数でない（{num}/{den}）"
            )));
        }
        let g = gcd(num as u128, den as u128) as u32;
        Ok(Self {
            num: num / g,
            den: den / g,
        })
    }

    /// 整数の fps。定数で 0 を渡すとコンパイルエラー。実行時の値は [`Rational::new`] で作る。
    ///
    /// # Panics
    /// `n` が 0 のとき。
    pub const fn integer(n: u32) -> Self {
        assert!(n > 0, "Rational::integer: 0 は正の数でない");
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
/// 分子の絶対値は `i64::MAX` 以下（文書に書く形 [`RawTime`] の分子が i64 のため）。
/// den が 2^32 でも約 68 年を表せる。範囲を超える演算は `Err`。
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
        Self {
            // 作るときに |num| <= i64::MAX を検査しているので丸めは起きない。
            num: t.num as i64,
            den: t.den,
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

    /// `num / den` 秒（約分する）。den が 0、または表せる範囲を超えれば `InvalidArgument`。
    pub fn new(num: i128, den: u32) -> Result<Self> {
        Self::reduced(num, den as u128)
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
    pub fn from_frames(frames: i64, fps: Rational) -> Result<Self> {
        Self::reduced(frames as i128 * fps.den as i128, fps.num as u128)
    }

    /// 約分して、分母が u32、分子が上限に収まるか確かめる。
    fn reduced(num: i128, den: u128) -> Result<Self> {
        if den == 0 {
            return Err(MmError::InvalidArgument("RationalTime: den が 0".into()));
        }
        let g = gcd(num.unsigned_abs(), den);
        let (num, den) = (num / g as i128, den / g);
        match u32::try_from(den) {
            Ok(den) if num.abs() <= MAX_NUM => Ok(Self { num, den }),
            _ => Err(MmError::InvalidArgument(format!(
                "RationalTime: {num}/{den} は表せる範囲を超えた"
            ))),
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

    /// 足す。最小公倍数の分母で計算し、表せなければ `Err`。
    pub fn checked_add(self, rhs: Self) -> Result<Self> {
        self.combine(rhs, 1)
    }

    /// 引く。最小公倍数の分母で計算し、表せなければ `Err`。
    pub fn checked_sub(self, rhs: Self) -> Result<Self> {
        self.combine(rhs, -1)
    }

    fn combine(self, rhs: Self, sign: i128) -> Result<Self> {
        let g = gcd(self.den as u128, rhs.den as u128);
        let lcm = self.den as u128 / g * rhs.den as u128;
        let left = (lcm / self.den as u128) as i128;
        let right = (lcm / rhs.den as u128) as i128;
        // 分子は i64 に収まり倍率は 2^64 未満なので、各項の積は i128 であふれない。
        let num = self.num * left + sign * rhs.num * right;
        Self::reduced(num, lcm)
    }

    /// 有理数倍（再生の速さ、fps）。表せなければ `Err`。
    pub fn scale(self, r: Rational) -> Result<Self> {
        Self::reduced(self.num * r.num as i128, self.den as u128 * r.den as u128)
    }

    /// 有理数で割る。表せなければ `Err`。
    pub fn div_by(self, r: Rational) -> Result<Self> {
        Self::reduced(self.num * r.den as i128, self.den as u128 * r.num as u128)
    }

    /// 素材のコマを選ぶ唯一の規則: `floor(t * fps + 1/2^20)`。
    /// 呼ぶ側は先に `t - offset` を素材の時間軸に写しておく。結果が i64 を超えるときは端に丸める。
    pub fn frame_index(self, fps: Rational) -> i64 {
        let shift = 1i128 << FRAME_SNAP_SHIFT;
        let den = self.den as i128 * fps.den as i128;
        let numer = self.num * fps.num as i128 * shift + den;
        let q = div_floor(numer, den * shift);
        i64::try_from(q).unwrap_or(if q < 0 { i64::MIN } else { i64::MAX })
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
    // 常に既約で持つので、そのまま畳めば等しい時刻は等しいハッシュになる。
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.num.hash(state);
        self.den.hash(state);
    }
}

impl fmt::Display for RationalTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}s", self.num, self.den)
    }
}

/// 半開区間 `[start, end)` の時刻の範囲。読み込みも [`TimeRange::new`] の検査を通る。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RawRange")]
pub struct TimeRange {
    pub start: RationalTime,
    pub end: RationalTime,
}

/// 文書に書く形の範囲。
#[derive(Deserialize)]
struct RawRange {
    start: RationalTime,
    end: RationalTime,
}

impl TryFrom<RawRange> for TimeRange {
    type Error = MmError;

    fn try_from(raw: RawRange) -> Result<Self> {
        Self::new(raw.start, raw.end)
    }
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

    /// 始まりと長さから。長さが負、または表せなければ `Err`。
    pub fn from_start_duration(start: RationalTime, duration: RationalTime) -> Result<Self> {
        Self::new(start, start.checked_add(duration)?)
    }

    /// 長さ。表せなければ `Err`。
    pub fn duration(&self) -> Result<RationalTime> {
        self.end.checked_sub(self.start)
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
        let s = t(1, 6).checked_add(t(1, 3)).unwrap();
        assert_eq!((s.num(), s.den()), (1, 2));
        let d = t(1, 2).checked_sub(t(1, 2)).unwrap();
        assert_eq!(d, RationalTime::ZERO);
    }

    #[test]
    fn 分母の積が_u32_を超えても最小公倍数で足せる() {
        // 90kHz と 48kHz の積は 4.32e9 で u32 を超えるが、最小公倍数は 720000。
        let s = t(1, 90_000).checked_add(t(1, 48_000)).unwrap();
        assert_eq!((s.num(), s.den()), (23, 720_000));
        let d = t(1, 90_000).checked_sub(t(1, 48_000)).unwrap();
        assert_eq!((d.num(), d.den()), (-7, 720_000));
    }

    #[test]
    fn 分母が_u32_に収まらない結果は_err() {
        // 互いに素な大きい分母の和は 2^32 を超える。
        let a = t(1, 4_294_967_295);
        let b = t(1, 4_294_967_291);
        assert!(a.checked_add(b).is_err());
        assert!(a.scale(Rational::new(1, 4_294_967_291).unwrap()).is_err());
    }

    #[test]
    fn 分子が大きすぎる結果は_err() {
        let big = RationalTime::seconds(i64::MAX);
        assert!(big.checked_add(big).is_err());
        assert!(RationalTime::seconds(i64::MAX)
            .scale(Rational::integer(2))
            .is_err());
        assert!(RationalTime::new(i128::MAX, 1).is_err());
    }

    #[test]
    fn 速さで倍して割る() {
        let speed = Rational::new(3, 2).unwrap();
        assert_eq!(t(1, 1).scale(speed).unwrap(), t(3, 2));
        assert_eq!(t(3, 2).div_by(speed).unwrap(), t(1, 1));
    }

    #[test]
    fn コマ境界ちょうどは次のコマになる() {
        let fps = Rational::new(30000, 1001).unwrap();
        for i in 0..10_000 {
            let time = RationalTime::from_frames(i, fps).unwrap();
            assert_eq!(time.frame_index(fps), i);
        }
    }

    #[test]
    fn コマ境界の少し手前は前のコマになる() {
        let fps = Rational::integer(60);
        let eps = t(1, 1_000_000);
        let time = RationalTime::from_frames(10, fps)
            .unwrap()
            .checked_sub(eps)
            .unwrap();
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
        assert_eq!(a.intersect(&b).unwrap().duration().unwrap(), t(1, 1));
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

    #[test]
    fn 等しい時刻は等しいハッシュになる() {
        use std::hash::{BuildHasher, RandomState};
        let rs = RandomState::new();
        assert_eq!(rs.hash_one(t(1, 2)), rs.hash_one(t(2, 4)));
    }

    #[test]
    fn rational_は読み込みでも検査と約分を通る() {
        assert!(serde_json::from_str::<Rational>(r#"{"num":30,"den":0}"#).is_err());
        assert!(serde_json::from_str::<Rational>(r#"{"num":0,"den":1}"#).is_err());
        let r: Rational = serde_json::from_str(r#"{"num":60,"den":2}"#).unwrap();
        assert_eq!(r, Rational::integer(30));
    }

    #[test]
    fn rational_は_0_を作れない() {
        assert!(Rational::new(0, 1).is_err());
        assert!(Rational::new(1, 0).is_err());
    }

    #[test]
    fn 範囲は読み込みでも逆順を断る() {
        let s = r#"{"start":{"num":2,"den":1},"end":{"num":1,"den":1}}"#;
        assert!(serde_json::from_str::<TimeRange>(s).is_err());
    }
}
