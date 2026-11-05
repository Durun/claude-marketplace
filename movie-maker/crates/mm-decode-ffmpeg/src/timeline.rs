//! 映像の時間軸。パケットの PTS を先に全部読み、コマの番号を PTS の位置で決める。
//!
//! 番号を「時刻 × fps」から逆算せず索引の位置にするのは、B フレームや VFR でも
//! デコーダーが返す順序に依らず、シーク後も同じ番号が付くため。

use ffmpeg_next as ff;
use mm_types::{MmError, Rational, RationalTime, Result};

/// ストリームの時間の単位（`num / den` 秒）。
#[derive(Debug, Clone, Copy)]
pub struct TimeBase {
    pub num: i64,
    pub den: i64,
}

impl TimeBase {
    /// ストリームの time_base から。分母か分子が 0 以下なら壊れた素材。
    pub fn new(tb: ff::Rational) -> Result<Self> {
        let (num, den) = (tb.numerator() as i64, tb.denominator() as i64);
        if num <= 0 || den <= 0 {
            return Err(MmError::Corrupt(format!("time_base が不正: {num}/{den}")));
        }
        Ok(Self { num, den })
    }

    /// ticks を秒の有理数にする。
    pub fn time(self, ticks: i64) -> Result<RationalTime> {
        let den = u32::try_from(self.den).map_err(|_| {
            MmError::Unsupported(format!("time_base の分母が大きすぎる: {}", self.den))
        })?;
        RationalTime::new(ticks as i128 * self.num as i128, den)
    }

    /// 秒の有理数を ticks に切り下げる。
    pub fn ticks_floor(self, t: RationalTime) -> i64 {
        let numer = t.num() * self.den as i128;
        let denom = t.den() as i128 * self.num as i128;
        numer.div_euclid(denom) as i64
    }

    /// ticks をサンプル数に丸める（音の位置の計算用）。
    pub fn samples_round(self, ticks: i64, rate: u32) -> i64 {
        let numer = ticks as i128 * self.num as i128 * rate as i128 * 2 + self.den as i128;
        numer.div_euclid(self.den as i128 * 2) as i64
    }
}

/// 映像ストリームのパケットを全部読んで得た索引。
#[derive(Debug)]
pub struct VideoScan {
    /// 昇順で重複のない PTS（ストリームの ticks）。
    pub raw_pts: Vec<i64>,
    /// 最後のコマの長さ（ticks）。分からなければ 0。
    pub last_duration: i64,
}

/// 映像ストリームの PTS を全部読む。パケットが 1 つも無ければ `Corrupt`。
pub fn scan_video(path: &std::path::Path, stream_index: usize) -> Result<VideoScan> {
    let mut input = ff::format::input(&path)
        .map_err(|e| MmError::Corrupt(format!("{}: {e}", path.display())))?;
    let mut pts = Vec::new();
    let mut packet = ff::Packet::empty();
    loop {
        match packet.read(&mut input) {
            Ok(()) => {
                if packet.stream() != stream_index {
                    continue;
                }
                if let Some(p) = packet.pts().or(packet.dts()) {
                    pts.push((p, packet.duration()));
                }
            }
            Err(ff::Error::Eof) => break,
            Err(ff::Error::InvalidData) => continue,
            // 途中で切れた素材は読めた所までを索引にする
            Err(_) => break,
        }
    }
    if pts.is_empty() {
        return Err(MmError::Corrupt(format!(
            "{}: 映像のパケットが無い",
            path.display()
        )));
    }
    pts.sort_by_key(|(p, _)| *p);
    pts.dedup_by_key(|(p, _)| *p);
    let last_duration = pts.last().map_or(0, |(_, d)| (*d).max(0));
    Ok(VideoScan {
        raw_pts: pts.into_iter().map(|(p, _)| p).collect(),
        last_duration,
    })
}

/// PTS の列が固定フレームレートなら、その fps。
///
/// 候補（ストリームの宣言、PTS の最初の間隔）のうち、全コマが 1 tick 以内で
/// `i / fps` に載るものを採る。mkv の ms 単位のような丸めの揺れは CFR と見なす。
pub fn detect_fps(raw: &[i64], tb: TimeBase, declared: &[ff::Rational]) -> Option<Rational> {
    let mut candidates: Vec<Rational> = declared.iter().filter_map(|r| to_rational(*r)).collect();
    if raw.len() >= 2 {
        let diff = (raw[1] - raw[0]) as i128;
        let num = tb.den as i128;
        let den = tb.num as i128 * diff;
        if let (Ok(n), Ok(d)) = (u32::try_from(num), u32::try_from(den)) {
            if let Ok(r) = Rational::new(n, d) {
                candidates.push(r);
            }
        }
    }
    candidates.into_iter().find(|fps| fits(raw, tb, *fps))
}

fn to_rational(r: ff::Rational) -> Option<Rational> {
    let (n, d) = (r.numerator(), r.denominator());
    if n <= 0 || d <= 0 {
        return None;
    }
    Rational::new(n as u32, d as u32).ok()
}

fn fits(raw: &[i64], tb: TimeBase, fps: Rational) -> bool {
    let tick = tb.num as i128 * fps.num() as i128;
    raw.iter().enumerate().all(|(i, p)| {
        let actual = (*p - raw[0]) as i128 * tick;
        let expected = i as i128 * tb.den as i128 * fps.den() as i128;
        (actual - expected).abs() <= tick
    })
}
