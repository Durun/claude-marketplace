//! 映像の時間軸。パケットの PTS を先に全部読み、フレームの番号を PTS の位置で決める。
//!
//! 番号を「時刻 × fps」から逆算せず索引の位置にするのは、B フレームや VFR でも
//! デコーダーが返す順序に依らず、シーク後も同じ番号が付くため。

use ff::packet::Ref as _;
use ffmpeg_next as ff;
use mm_decode_api::MediaInfo;
use mm_types::{ColorSpace, MmError, Rational, RationalTime, Result, Size};

/// PTS の索引を作れなかった素材の 1 フレームの長さの仮定（秒）。フレームが 1 つだけのときに使う。
const FALLBACK_FRAME_SECONDS: (i128, u32) = (1, 30);

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
    /// 最後のフレームの長さ（ticks）。分からなければ 0。
    pub last_duration: i64,
    /// キーフレームのフレームの番号（昇順）。
    pub keyframes: Vec<i64>,
}

/// 映像ストリームの PTS を全部読む。パケットが 1 つも無ければ `Corrupt`。
///
/// MP4 の edit list の外のパケット（`AV_PKT_FLAG_DISCARD`）はデコードしても画像が出ないので、
/// 索引に入れない。入れるとフレームの番号が出るフレームからずれる。
pub fn scan_video(path: &std::path::Path, stream_index: usize) -> Result<VideoScan> {
    let mut input = ff::format::input(&path)
        .map_err(|e| MmError::Corrupt(format!("{}: {e}", path.display())))?;
    let mut pts = Vec::new();
    let mut key_pts = Vec::new();
    let mut packet = ff::Packet::empty();
    loop {
        match packet.read(&mut input) {
            Ok(()) => {
                if packet.stream() != stream_index || is_discarded(&packet) {
                    continue;
                }
                if let Some(p) = packet.pts().or(packet.dts()) {
                    pts.push((p, packet.duration()));
                    if packet.is_key() {
                        key_pts.push(p);
                    }
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
    let raw_pts: Vec<i64> = pts.into_iter().map(|(p, _)| p).collect();
    let mut keyframes: Vec<i64> = key_pts
        .iter()
        .filter_map(|p| raw_pts.binary_search(p).ok())
        .map(|i| i as i64)
        .collect();
    keyframes.sort_unstable();
    keyframes.dedup();
    Ok(VideoScan {
        raw_pts,
        last_duration,
        keyframes,
    })
}

fn is_discarded(packet: &ff::Packet) -> bool {
    // ffmpeg-next の `Flags` は DISCARD を持たず、`flags()` が未知のビットを破棄するので生で読む
    let raw = unsafe { (*packet.as_ptr()).flags };
    raw & ff::ffi::AV_PKT_FLAG_DISCARD != 0
}

/// 次に出すフレーム `next` の位置から `target` へ、シークせず読み進めてよいか。
/// `next..=target` の間に（`next` の次以降の）キーフレームが無ければ、同じ GOP の中なので読み進めるほうが速い。
pub fn can_read_forward(keyframes: &[i64], next: i64, target: i64) -> bool {
    if target < next {
        return false;
    }
    let after = keyframes.partition_point(|k| *k <= next);
    keyframes.get(after).is_none_or(|k| *k > target)
}

/// PTS の列が固定フレームレートなら、その fps。
///
/// 候補（ストリームの宣言、PTS の最初の間隔）のうち、全フレームが 1 tick 以内で
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

/// VFR の素材の、全フレームの時刻（先頭が 0）と全体の長さ。最後のフレームの長さは、パケットの長さ、
/// 直前との間隔、仮定の順に決める。
pub fn vfr_index(scan: &VideoScan, tb: TimeBase) -> Result<(RationalTime, Vec<RationalTime>)> {
    let raw = &scan.raw_pts;
    let index = raw
        .iter()
        .map(|p| tb.time(p - raw[0]))
        .collect::<Result<Vec<_>>>()?;
    let tail = match (scan.last_duration, raw.len()) {
        (d, _) if d > 0 => tb.time(d)?,
        (_, n) if n >= 2 => tb.time(raw[n - 1] - raw[n - 2])?,
        _ => RationalTime::new(FALLBACK_FRAME_SECONDS.0, FALLBACK_FRAME_SECONDS.1)?,
    };
    let last = *index.last().expect("raw は空でない");
    Ok((last.checked_add(tail)?, index))
}

/// 素材の時間軸。フレームの番号と PTS の対応、キーフレーム、`MediaInfo`。
#[derive(Debug)]
pub struct Timeline {
    /// 昇順で重複のない映像の PTS（ストリームの ticks）。位置がフレームの番号。
    pub raw_pts: Vec<i64>,
    /// キーフレームのフレームの番号（昇順）。シークするか読み進めるかの判断に使う。
    pub keyframes: Vec<i64>,
    /// CFR のとき、フレームの時刻を `index / fps` にする。
    pub fps: Option<Rational>,
    /// 素材の時間軸の 0（最初のフレーム）の秒。
    pub origin: RationalTime,
    pub info: MediaInfo,
}

impl Timeline {
    /// 素材を走査して作る。`size` と `color_space` はデコーダーの宣言から。
    pub fn build(
        path: &std::path::Path,
        stream: &ff::format::stream::Stream<'_>,
        size: Size,
        color_space: ColorSpace,
    ) -> Result<Self> {
        let tb = TimeBase::new(stream.time_base())?;
        let declared = [stream.avg_frame_rate(), stream.rate()];
        let scan = crate::index_cache::scan_video_cached(path, stream.index())?;
        let fps = detect_fps(&scan.raw_pts, tb, &declared);
        let (duration, pts_index) = match fps {
            Some(fps) => (
                RationalTime::from_frames(scan.raw_pts.len() as i64, fps)?,
                Vec::new(),
            ),
            None => vfr_index(&scan, tb)?,
        };
        let info = MediaInfo {
            duration,
            fps,
            size,
            color_space,
            timebase_offset: RationalTime::ZERO,
            audio: None,
            pts_index,
        };
        Ok(Self {
            origin: tb.time(scan.raw_pts[0])?,
            raw_pts: scan.raw_pts,
            keyframes: scan.keyframes,
            fps,
            info,
        })
    }

    /// フレームの数。
    pub fn count(&self) -> i64 {
        self.raw_pts.len() as i64
    }

    /// 番号のフレームの時刻。
    pub fn pts_of(&self, index: i64) -> Result<RationalTime> {
        match self.fps {
            Some(fps) => RationalTime::from_frames(index, fps),
            None => Ok(self.info.pts_index[index as usize]),
        }
    }

    /// PTS(ticks) のフレームの番号。索引に無ければ、それ以下で最後のフレーム。
    pub fn index_of(&self, pts: i64) -> i64 {
        match self.raw_pts.binary_search(&pts) {
            Ok(i) => i as i64,
            Err(i) => i.saturating_sub(1) as i64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: TimeBase = TimeBase { num: 1, den: 1000 };

    #[test]
    fn 同じ_gop_の前方は読み進める() {
        let keys = [0, 12, 24];
        assert!(can_read_forward(&keys, 3, 3));
        assert!(can_read_forward(&keys, 3, 11));
        assert!(can_read_forward(&keys, 12, 23));
        assert!(can_read_forward(&keys, 30, 59));
    }

    #[test]
    fn キーフレームをまたぐ前方と後戻りはシークする() {
        let keys = [0, 12, 24];
        assert!(!can_read_forward(&keys, 3, 12));
        assert!(!can_read_forward(&keys, 3, 40));
        assert!(!can_read_forward(&keys, 5, 4));
    }

    #[test]
    fn ms_単位の丸めの揺れは_cfr_と見なす() {
        let raw = [0, 33, 67, 100, 133, 167];
        let fps = detect_fps(&raw, MS, &[ff::Rational::new(30, 1)]);
        assert_eq!(fps, Some(Rational::integer(30)));
    }

    #[test]
    fn 間隔が揺れる列は_vfr_になる() {
        let raw = [0, 33, 100, 133, 300];
        assert_eq!(detect_fps(&raw, MS, &[ff::Rational::new(30, 1)]), None);
    }

    #[test]
    fn vfr_の長さは最後のフレームの長さを足す() {
        let scan = VideoScan {
            raw_pts: vec![0, 100, 300],
            last_duration: 0,
            keyframes: vec![0],
        };
        let (duration, index) = vfr_index(&scan, MS).unwrap();
        assert_eq!(index.len(), 3);
        // 長さが分からなければ直前との間隔（200 ms）を足す
        assert_eq!(duration, RationalTime::new(500, 1000).unwrap());
    }
}
