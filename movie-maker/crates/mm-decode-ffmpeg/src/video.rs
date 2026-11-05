//! 映像のデコーダー本体。

use std::path::{Path, PathBuf};

use ff::format::context::Input;
use ff::{frame, Packet};
use ffmpeg_next as ff;
use mm_audio_api::{AudioBlock, DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};
use mm_decode_api::{AudioStreamInfo, DecodedFrame, Decoder, MediaInfo};
use mm_types::{MmError, Rational, RationalTime, Result, Size, TimeRange};

use crate::audio::{self, AudioProbe, AudioReader};
use crate::convert::{Matrix, RgbaConverter};
use crate::timeline::{can_read_forward, detect_fps, scan_video, TimeBase};

/// PTS の索引を作れなかった素材の 1 コマの長さの仮定（秒）。コマが 1 つだけのときに使う。
const FALLBACK_FRAME_SECONDS: (i128, u32) = (1, 30);

/// FFmpeg で開いた素材 1 本。
pub struct FfmpegDecoder {
    path: PathBuf,
    info: MediaInfo,
    input: Input,
    stream_index: usize,
    decoder: ff::decoder::Video,
    /// 昇順で重複のない映像の PTS（ストリームの ticks）。位置がコマの番号。
    raw_pts: Vec<i64>,
    /// キーフレームのコマの番号（昇順）。シークするか読み進めるかの判断に使う。
    keyframes: Vec<i64>,
    /// CFR のとき、コマの時刻を `index / fps` にする。
    fps: Option<Rational>,
    converter: RgbaConverter,
    /// 次に出すコマの番号。
    next_index: i64,
    /// 復号済みで、まだ出していないコマ。
    held: Option<(i64, frame::Video)>,
    draining: bool,
    ended: bool,
    audio_probe: Option<AudioProbe>,
    audio: Option<AudioReader>,
    /// 素材の時間軸の 0（最初のコマ）の秒。
    origin: RationalTime,
}

impl FfmpegDecoder {
    /// 素材を開く。ファイルが無ければ `NotFound`、読めなければ `Corrupt`。
    pub fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(MmError::NotFound(path.display().to_string()));
        }
        crate::init()?;
        let input = ff::format::input(&path)
            .map_err(|e| MmError::Corrupt(format!("{}: {e}", path.display())))?;
        let stream = input
            .streams()
            .best(ff::media::Type::Video)
            .ok_or_else(|| MmError::Unsupported(format!("{}: 映像が無い", path.display())))?;
        let stream_index = stream.index();
        let tb = TimeBase::new(stream.time_base())?;
        let declared = [stream.avg_frame_rate(), stream.rate()];
        let mut ctx = ff::codec::Context::from_parameters(stream.parameters())
            .map_err(|e| MmError::Unsupported(format!("映像のコーデック: {e}")))?;
        // 1 スレッドだと 1080p の復号が数倍遅い。0 は FFmpeg が CPU の数で決める。
        ctx.set_threading(ff::threading::Config {
            kind: ff::threading::Type::Frame,
            ..Default::default()
        });
        let decoder = ctx
            .decoder()
            .video()
            .map_err(|e| MmError::Corrupt(format!("{}: 映像のデコーダー: {e}", path.display())))?;
        let audio_probe = audio::probe(&input)?;

        let scan = scan_video(path, stream_index)?;
        let origin = tb.time(scan.raw_pts[0])?;
        let fps = detect_fps(&scan.raw_pts, tb, &declared);
        let info = build_info(
            &decoder,
            &scan.raw_pts,
            scan.last_duration,
            tb,
            fps,
            audio_probe,
            origin,
        )?;
        Ok(Self {
            path: path.to_owned(),
            info,
            input,
            stream_index,
            decoder,
            raw_pts: scan.raw_pts,
            keyframes: scan.keyframes,
            fps,
            converter: RgbaConverter::default(),
            next_index: 0,
            held: None,
            draining: false,
            ended: false,
            audio_probe,
            audio: None,
            origin,
        })
    }

    fn count(&self) -> i64 {
        self.raw_pts.len() as i64
    }

    /// 番号のコマの時刻。
    fn pts_of(&self, index: i64) -> Result<RationalTime> {
        match self.fps {
            Some(fps) => RationalTime::from_frames(index, fps),
            None => Ok(self.info.pts_index[index as usize]),
        }
    }

    /// 復号器から次のコマを 1 つ取り出し、PTS の索引で番号を付ける。
    fn receive_indexed(&mut self) -> Result<Option<(i64, frame::Video)>> {
        let mut packet = Packet::empty();
        loop {
            let mut decoded = frame::Video::empty();
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    // PTS の無いコマは番号を決められないので読み飛ばす
                    if let Some(p) = decoded.timestamp().or(decoded.pts()) {
                        return Ok(Some((self.index_of(p), decoded)));
                    }
                    continue;
                }
                Err(ff::Error::Eof) => return Ok(None),
                Err(ff::Error::Other { errno }) if errno == ff::error::EAGAIN => {}
                Err(e) => return Err(MmError::Corrupt(format!("映像の復号: {e}"))),
            }
            if self.draining {
                return Ok(None);
            }
            match packet.read(&mut self.input) {
                Ok(()) if packet.stream() == self.stream_index => {
                    match self.decoder.send_packet(&packet) {
                        Ok(()) | Err(ff::Error::InvalidData) => {}
                        Err(e) => return Err(MmError::Corrupt(format!("映像のパケット: {e}"))),
                    }
                }
                Ok(()) | Err(ff::Error::InvalidData) => {}
                Err(ff::Error::Eof) => {
                    let _ = self.decoder.send_eof();
                    self.draining = true;
                }
                Err(e) => return Err(MmError::Corrupt(format!("映像の読み込み: {e}"))),
            }
        }
    }

    /// PTS(ticks) のコマの番号。索引に無ければ、それ以下で最後のコマ。
    fn index_of(&self, pts: i64) -> i64 {
        match self.raw_pts.binary_search(&pts) {
            Ok(i) => i as i64,
            Err(i) => i.saturating_sub(1) as i64,
        }
    }

    /// 同じ GOP の中で、復号器を進めながら `index` より前のコマを捨てる。
    /// シークは次のキーフレームへ戻って復号し直すので、ここで読み進めるほうが少ない仕事で済む。
    fn skip_to(&mut self, index: i64) -> Result<()> {
        self.next_index = index;
        loop {
            let next = match self.held.take() {
                Some(h) => Some(h),
                None => self.receive_indexed()?,
            };
            match next {
                Some((i, _)) if i < index => {}
                Some(first) => {
                    self.held = Some(first);
                    return Ok(());
                }
                None => {
                    self.ended = true;
                    return Ok(());
                }
            }
        }
    }

    /// 映像ストリームの `ts`（ticks）以前のキーフレームへ戻し、復号器を空にする。
    fn reposition(&mut self, ts: i64) -> Result<()> {
        unsafe {
            let ret = ff::ffi::av_seek_frame(
                self.input.as_mut_ptr(),
                self.stream_index as i32,
                ts,
                ff::ffi::AVSEEK_FLAG_BACKWARD,
            );
            if ret < 0 {
                return Err(MmError::Corrupt(format!(
                    "{}: シーク: {}",
                    self.path.display(),
                    ff::Error::from(ret)
                )));
            }
        }
        self.decoder.flush();
        self.draining = false;
        self.ended = false;
        self.held = None;
        Ok(())
    }
}

impl Decoder for FfmpegDecoder {
    fn info(&self) -> &MediaInfo {
        &self.info
    }

    fn seek(&mut self, t: RationalTime) -> Result<()> {
        let count = self.count();
        let index = self.info.frame_index_at(t).clamp(0, count);
        if index == self.next_index {
            return Ok(());
        }
        if index < count && !self.ended && can_read_forward(&self.keyframes, self.next_index, index)
        {
            return self.skip_to(index);
        }
        self.held = None;
        self.next_index = index;
        if index >= count {
            self.ended = true;
            return Ok(());
        }
        // コンテナによってはキーフレームの PTS が目的のコマより後に見えるので、
        // 先頭のコマが目的を過ぎたら、手前のコマの PTS を指して探し直す。
        let mut margin = 0;
        loop {
            let seek_index = index - margin;
            self.reposition(self.raw_pts[seek_index as usize])?;
            match self.receive_indexed()? {
                Some((i, f)) if i > index && seek_index > 0 => {
                    drop(f);
                    margin = if margin == 0 { 1 } else { margin * 4 }.min(index);
                }
                Some(first) => {
                    self.held = Some(first);
                    return Ok(());
                }
                None => {
                    self.ended = true;
                    return Ok(());
                }
            }
        }
    }

    fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        loop {
            if self.next_index >= self.count() || (self.ended && self.held.is_none()) {
                return Ok(None);
            }
            let next = match self.held.take() {
                Some(h) => Some(h),
                None => self.receive_indexed()?,
            };
            let Some((index, raw)) = next else {
                self.ended = true;
                return Ok(None);
            };
            if index < self.next_index {
                continue;
            }
            let image = self.converter.convert(&raw)?;
            self.next_index = index + 1;
            return Ok(Some(DecodedFrame::cpu(index, self.pts_of(index)?, image)));
        }
    }

    fn decode_audio(&mut self, range: TimeRange) -> Result<AudioBlock> {
        let Some(probe) = self.audio_probe else {
            let frames = (range
                .end
                .frame_index(Rational::integer(DEFAULT_SAMPLE_RATE))
                - range
                    .start
                    .frame_index(Rational::integer(DEFAULT_SAMPLE_RATE)))
            .max(0) as usize;
            return Ok(AudioBlock::silence(
                DEFAULT_SAMPLE_RATE,
                DEFAULT_CHANNELS,
                frames,
            ));
        };
        if self.audio.is_none() {
            self.audio = Some(AudioReader::open(&self.path, probe, self.origin)?);
        }
        self.audio.as_mut().expect("直前に開いた").read(range)
    }
}

fn build_info(
    decoder: &ff::decoder::Video,
    raw: &[i64],
    last_duration: i64,
    tb: TimeBase,
    fps: Option<Rational>,
    audio: Option<AudioProbe>,
    origin: RationalTime,
) -> Result<MediaInfo> {
    let (duration, pts_index) = match fps {
        Some(fps) => (
            RationalTime::from_frames(raw.len() as i64, fps)?,
            Vec::new(),
        ),
        None => {
            let index = raw
                .iter()
                .map(|p| tb.time(p - raw[0]))
                .collect::<Result<Vec<_>>>()?;
            let tail = match (last_duration, raw.len()) {
                (d, _) if d > 0 => tb.time(d)?,
                (_, n) if n >= 2 => tb.time(raw[n - 1] - raw[n - 2])?,
                _ => RationalTime::new(FALLBACK_FRAME_SECONDS.0, FALLBACK_FRAME_SECONDS.1)?,
            };
            let last = *index.last().expect("raw は空でない");
            (last.checked_add(tail)?, index)
        }
    };
    let matrix = Matrix::from_frame(decoder.color_space(), decoder.height());
    let timebase_offset = match audio {
        Some(a) => origin.checked_sub(a.start)?,
        None => RationalTime::ZERO,
    };
    Ok(MediaInfo {
        duration,
        fps,
        size: Size::new(decoder.width(), decoder.height()),
        color_space: matrix.color_space(),
        timebase_offset,
        // 音は素材の形によらず、読むときに既定のレートとチャンネル数へそろえる
        audio: audio.map(|_| AudioStreamInfo {
            sample_rate: DEFAULT_SAMPLE_RATE,
            channels: DEFAULT_CHANNELS,
        }),
        pts_index,
    })
}
