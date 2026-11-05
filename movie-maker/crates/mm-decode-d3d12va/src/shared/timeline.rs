//! 映像の時間軸。パケットの PTS を先に全部読み、コマの番号を PTS の位置で決める。
//! 番号の付け方は `mm-decode-ffmpeg` と同じで、同じ素材は CPU の経路と同じ番号になる。

use ff::packet::Ref as _;
use ffmpeg_next as ff;
use mm_decode_api::MediaInfo;
use mm_types::{ColorSpace, MmError, Rational, RationalTime, Result, Size};

/// PTS の索引を作れなかった素材の 1 コマの長さの仮定（秒）。コマが 1 つだけのときに使う。
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
/// MP4 の edit list の外のパケット（`AV_PKT_FLAG_DISCARD`）は絵が出ないので索引に入れない。
pub fn scan_video(path: &std::path::Path, stream_index: usize) -> Result<VideoScan> {
    let mut input = ff::format::input(&path)
        .map_err(|e| MmError::Corrupt(format!("{}: {e}", path.display())))?;
    let mut pts = Vec::new();
    let mut packet = ff::Packet::empty();
    loop {
        match packet.read(&mut input) {
            Ok(()) => {
                if packet.stream() != stream_index || is_discarded(&packet) {
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

fn is_discarded(packet: &ff::Packet) -> bool {
    // ffmpeg-next の `Flags` は DISCARD を持たず未知のビットを落とすので生で読む
    let raw = unsafe { (*packet.as_ptr()).flags };
    raw & ff::ffi::AV_PKT_FLAG_DISCARD != 0
}

/// PTS の列が固定フレームレートなら、その fps。
/// 宣言と最初の間隔のうち、全コマが 1 tick 以内で `i / fps` に載るものを採る。
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

/// 素材の時間軸。コマの番号と PTS の対応、`MediaInfo`。
#[derive(Debug)]
pub struct Timeline {
    pub raw_pts: Vec<i64>,
    pub fps: Option<Rational>,
    pub info: MediaInfo,
}

impl Timeline {
    /// 素材を走査して作る。`size` と `color_space` は復号器の宣言から。
    pub fn build(
        path: &std::path::Path,
        stream: &ff::format::stream::Stream<'_>,
        size: Size,
        color_space: ColorSpace,
    ) -> Result<Self> {
        let tb = TimeBase::new(stream.time_base())?;
        let declared = [stream.avg_frame_rate(), stream.rate()];
        let scan = scan_video(path, stream.index())?;
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
            raw_pts: scan.raw_pts,
            fps,
            info,
        })
    }

    /// コマの数。
    pub fn count(&self) -> i64 {
        self.raw_pts.len() as i64
    }

    /// 番号のコマの時刻。
    pub fn pts_of(&self, index: i64) -> Result<RationalTime> {
        match self.fps {
            Some(fps) => RationalTime::from_frames(index, fps),
            None => Ok(self.info.pts_index[index as usize]),
        }
    }

    /// PTS(ticks) のコマの番号。索引に無ければ、それ以下で最後のコマ。
    pub fn index_of(&self, pts: i64) -> i64 {
        match self.raw_pts.binary_search(&pts) {
            Ok(i) => i as i64,
            Err(i) => i.saturating_sub(1) as i64,
        }
    }
}

fn vfr_index(scan: &VideoScan, tb: TimeBase) -> Result<(RationalTime, Vec<RationalTime>)> {
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
