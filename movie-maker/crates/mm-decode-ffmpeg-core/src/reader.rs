//! デコーダーからフレームを取り出す層。パケットの読み込み、PTS の索引による番号づけ、シーク。

use std::path::{Path, PathBuf};

use ff::format::context::Input;
use ff::{frame, Packet};
use ffmpeg_next as ff;
use mm_decode_api::{AudioStreamInfo, MediaInfo};
use mm_types::{MmError, RationalTime, Result, Size};

use crate::matrix::{check_transfer, Matrix};
use crate::timeline::{can_read_forward, Timeline};

/// 開いた入力と、映像のデコーダー。パケットを送ってフレームを受ける。
pub struct StreamReader {
    input: Input,
    stream_index: usize,
    decoder: ff::decoder::Video,
    draining: bool,
}

impl StreamReader {
    /// 開いた入力とデコーダーを組にする。
    pub fn new(input: Input, stream_index: usize, decoder: ff::decoder::Video) -> Self {
        Self {
            input,
            stream_index,
            decoder,
            draining: false,
        }
    }

    /// 映像ストリーム。
    pub fn stream(&self) -> ff::format::stream::Stream<'_> {
        self.input
            .stream(self.stream_index)
            .expect("開いた映像ストリーム")
    }

    /// デコーダーの宣言。
    pub fn decoder(&self) -> &ff::decoder::Video {
        &self.decoder
    }

    /// 次のデコード済みのフレーム。終わりなら `None`。
    pub fn receive(&mut self) -> Result<Option<frame::Video>> {
        let mut packet = Packet::empty();
        loop {
            let mut decoded = frame::Video::empty();
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => return Ok(Some(decoded)),
                Err(ff::Error::Eof) => return Ok(None),
                Err(ff::Error::Other { errno }) if errno == ff::error::EAGAIN => {}
                Err(e) => return Err(MmError::Corrupt(format!("映像のデコード: {e}"))),
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

    /// ストリームの `ts`（ticks）以前のキーフレームへ戻し、デコーダーを空にする。
    pub fn reposition(&mut self, ts: i64) -> Result<()> {
        // SAFETY: input は開いている。ストリーム番号は open で得たもの。
        let ret = unsafe {
            ff::ffi::av_seek_frame(
                self.input.as_mut_ptr(),
                self.stream_index as i32,
                ts,
                ff::ffi::AVSEEK_FLAG_BACKWARD,
            )
        };
        if ret < 0 {
            return Err(MmError::Corrupt(format!(
                "シーク: {}",
                ff::Error::from(ret)
            )));
        }
        self.decoder.flush();
        self.draining = false;
        Ok(())
    }
}

/// 番号と時刻が付いたフレーム。
pub struct IndexedFrame {
    pub index: i64,
    pub pts: RationalTime,
    pub frame: frame::Video,
}

/// フレームを番号の順に出す。シークは目的のフレームの手前のキーフレームからデコードして捨てる。
pub struct Sequencer {
    reader: StreamReader,
    timeline: Timeline,
    path: PathBuf,
    next_index: i64,
    held: Option<(i64, frame::Video)>,
    ended: bool,
}

impl Sequencer {
    /// PTS を走査して時間軸を作る。PQ と HLG の素材は `Unsupported`。
    pub fn new(path: &Path, reader: StreamReader) -> Result<Self> {
        let dec = reader.decoder();
        check_transfer(dec.color_transfer_characteristic())?;
        let size = Size::new(dec.width(), dec.height());
        let matrix = Matrix::from_frame(dec.color_space(), dec.height());
        let timeline = Timeline::build(path, &reader.stream(), size, matrix.color_space())?;
        Ok(Self {
            reader,
            timeline,
            path: path.to_owned(),
            next_index: 0,
            held: None,
            ended: false,
        })
    }

    /// 素材の情報。
    pub fn info(&self) -> &MediaInfo {
        &self.timeline.info
    }

    /// 素材の時間軸の 0（最初のフレーム）の秒。音の頭のずれを決めるのに使う。
    pub fn origin(&self) -> RationalTime {
        self.timeline.origin
    }

    /// 音のストリームの情報と、映像に対する音の頭のずれを素材の情報に入れる。
    pub fn set_audio(&mut self, audio: Option<AudioStreamInfo>, timebase_offset: RationalTime) {
        self.timeline.info.audio = audio;
        self.timeline.info.timebase_offset = timebase_offset;
    }

    fn receive_indexed(&mut self) -> Result<Option<(i64, frame::Video)>> {
        while let Some(decoded) = self.reader.receive()? {
            // PTS の無いフレームは番号を決められないので読み飛ばす
            if let Some(p) = decoded.timestamp().or(decoded.pts()) {
                return Ok(Some((self.timeline.index_of(p), decoded)));
            }
        }
        Ok(None)
    }

    /// 次のフレーム。終わりなら `None`。
    pub fn next_frame(&mut self) -> Result<Option<IndexedFrame>> {
        loop {
            if self.next_index >= self.timeline.count() || (self.ended && self.held.is_none()) {
                return Ok(None);
            }
            let next = match self.held.take() {
                Some(h) => Some(h),
                None => self.receive_indexed()?,
            };
            let Some((index, frame)) = next else {
                self.ended = true;
                return Ok(None);
            };
            if index < self.next_index {
                continue;
            }
            self.next_index = index + 1;
            let pts = self.timeline.pts_of(index)?;
            return Ok(Some(IndexedFrame { index, pts, frame }));
        }
    }

    /// `t` のフレームが次に出るようにする。同じ GOP の中の先なら読み進め、そうでなければ手前のキーフレームから探し直す。
    pub fn seek(&mut self, t: RationalTime) -> Result<()> {
        let count = self.timeline.count();
        let index = self.timeline.info.frame_index_at(t).clamp(0, count);
        if index == self.next_index {
            return Ok(());
        }
        if index < count
            && !self.ended
            && can_read_forward(&self.timeline.keyframes, self.next_index, index)
        {
            return self.skip_to(index);
        }
        self.held = None;
        self.next_index = index;
        self.ended = index >= count;
        if self.ended {
            return Ok(());
        }
        self.reposition_to(index)
    }

    /// デコーダーを進めながら `index` より前のフレームを捨てる。
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

    /// 目的のフレームの PTS を指して戻す。コンテナによってはキーフレームの PTS が目的より後に
    /// 見えるので、先頭のフレームが目的を過ぎたら手前のフレームの PTS で探し直す。
    fn reposition_to(&mut self, index: i64) -> Result<()> {
        let mut margin = 0;
        loop {
            let seek_index = index - margin;
            self.reader
                .reposition(self.timeline.raw_pts[seek_index as usize])
                .map_err(|e| MmError::Corrupt(format!("{}: {e}", self.path.display())))?;
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
}
