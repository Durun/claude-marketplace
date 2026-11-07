//! 音のデコード。映像とは別に素材を開き直し、映像のシークと干渉しないようにする。
//!
//! 出力は素材のレートとチャンネル数によらず 48 kHz・ステレオに固定する（ミキサーが形の違う
//! 入力を受けないため）。リサンプラーは 1 つで、デコーダーの位置が続く間は同じものを使い回す。

use std::path::Path;

use ff::format::sample::{Sample, Type};
use ff::software::resampling::Context as Resampler;
use ff::{frame, ChannelLayout, Packet};
use ffmpeg_next as ff;
use mm_audio_api::{AudioBlock, DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};
use mm_decode_ffmpeg_core::timeline::TimeBase;
use mm_types::{MmError, Rational, RationalTime, Result, TimeRange};

/// 区間の先頭より前からデコードを始める長さ（秒）。AAC などの前のウィンドウの分を破棄するため。
const PREROLL_SECONDS: i64 = 1;

/// 変換先のサンプル数に足す余裕。リサンプラーの内部の遅れで 1 回の出力が入力の換算より増えても収まる。
const OUTPUT_SLACK: usize = 512;

/// 直前の区間の続きを、シークせず読み進めてよい先の距離（出力のサンプル数）。1 秒。
const FORWARD_SAMPLES: i64 = DEFAULT_SAMPLE_RATE as i64;

/// 音のストリームを開いた状態。
pub struct AudioReader {
    input: ff::format::context::Input,
    stream_index: usize,
    decoder: ff::decoder::Audio,
    tb: TimeBase,
    /// 素材の時間軸の 0（映像の最初のフレーム）を、出力のサンプル数で表した値。
    origin_samples: i64,
    resampler: Option<Resampler>,
    /// 最後に変換したフレーム。次の区間が続きなら、ここから先を使う。
    chunk: Option<Chunk>,
    /// デコーダーが終端まで出し切った。
    eof: bool,
}

/// 変換済みの 1 フレーム分（48 kHz・ステレオ交互）。
struct Chunk {
    /// 素材の時間軸でのサンプルの位置。
    start: i64,
    samples: Vec<f32>,
}

impl Chunk {
    fn end(&self) -> i64 {
        self.start + (self.samples.len() / usize::from(DEFAULT_CHANNELS)) as i64
    }
}

/// 音のストリームの性質。
#[derive(Debug, Clone, Copy)]
pub struct AudioProbe {
    pub stream_index: usize,
    /// 音のストリームの最初の PTS（秒）。
    pub start: RationalTime,
}

/// 音のストリームがあれば性質を返す。
pub fn probe(input: &ff::format::context::Input) -> Result<Option<AudioProbe>> {
    let Some(stream) = input.streams().best(ff::media::Type::Audio) else {
        return Ok(None);
    };
    let tb = TimeBase::new(stream.time_base())?;
    let start_ticks = stream.start_time();
    let start = if start_ticks == ff::ffi::AV_NOPTS_VALUE {
        RationalTime::ZERO
    } else {
        tb.time(start_ticks)?
    };
    Ok(Some(AudioProbe {
        stream_index: stream.index(),
        start,
    }))
}

impl AudioReader {
    /// 素材を開き直して音のストリームをデコードできる状態にする。
    pub fn open(path: &Path, probe: AudioProbe, origin: RationalTime) -> Result<Self> {
        let input = ff::format::input(&path)
            .map_err(|e| MmError::Corrupt(format!("{}: {e}", path.display())))?;
        let stream = input
            .stream(probe.stream_index)
            .ok_or_else(|| MmError::Corrupt("音のストリームが消えた".into()))?;
        let tb = TimeBase::new(stream.time_base())?;
        let ctx = ff::codec::Context::from_parameters(stream.parameters())
            .map_err(|e| MmError::Unsupported(format!("音のコーデック: {e}")))?;
        let mut decoder = ctx.decoder();
        // デコーダーが前置き（AAC の priming）を捨てたとき、フレームの PTS を捨てた分だけ進めるのに要る。
        // 無いと、最初のパケットの途中までを捨てる素材で、音全体が捨てた分だけ早くなる。
        decoder.set_packet_time_base(stream.time_base());
        let decoder = decoder
            .audio()
            .map_err(|e| MmError::Unsupported(format!("音のデコーダー: {e}")))?;
        Ok(Self {
            input,
            stream_index: probe.stream_index,
            decoder,
            tb,
            origin_samples: round_to_samples(origin)?,
            resampler: None,
            chunk: None,
            eof: false,
        })
    }

    /// 素材の時間軸の区間の音。外は無音。
    pub fn read(&mut self, range: TimeRange) -> Result<AudioBlock> {
        let rate = Rational::new(DEFAULT_SAMPLE_RATE, 1)?;
        let first = range.start.frame_index(rate);
        let count = (range.end.frame_index(rate) - first).max(0) as usize;
        let channels = usize::from(DEFAULT_CHANNELS);
        let mut out = vec![0.0f32; count * channels];
        if count > 0 {
            self.fill(first, &mut out)?;
        }
        AudioBlock::from_samples(DEFAULT_SAMPLE_RATE, DEFAULT_CHANNELS, out)
    }

    fn fill(&mut self, first: i64, out: &mut [f32]) -> Result<()> {
        let channels = usize::from(DEFAULT_CHANNELS);
        let end = first + (out.len() / channels) as i64;
        if self.continues_at(first) {
            if let Some(chunk) = &self.chunk {
                copy_overlap(&chunk.samples, chunk.start, first, end, channels, out);
                if chunk.end() >= end {
                    return Ok(());
                }
            }
        } else {
            self.seek(first)?;
        }
        let mut packet = Packet::empty();
        while !self.eof {
            let mut decoded = frame::Audio::empty();
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    if self.accept(&mut decoded, first, end, out)? {
                        return Ok(());
                    }
                    continue;
                }
                Err(ff::Error::Eof) => self.eof = true,
                Err(ff::Error::Other { errno }) if errno == ff::error::EAGAIN => {
                    self.feed(&mut packet)?;
                }
                Err(e) => return Err(MmError::Corrupt(format!("音のデコード: {e}"))),
            }
        }
        Ok(())
    }

    /// パケットを 1 つデコーダーへ送る。終端なら終端を伝える。
    fn feed(&mut self, packet: &mut Packet) -> Result<()> {
        match packet.read(&mut self.input) {
            Ok(()) if packet.stream() == self.stream_index => {
                match self.decoder.send_packet(packet) {
                    Ok(()) | Err(ff::Error::InvalidData) => {}
                    Err(e) => return Err(MmError::Corrupt(format!("音のパケット: {e}"))),
                }
            }
            Ok(()) | Err(ff::Error::InvalidData) => {}
            Err(ff::Error::Eof) => {
                // 終端の読み残しを出させる
                let _ = self.decoder.send_eof();
            }
            Err(e) => return Err(MmError::Corrupt(format!("音の読み込み: {e}"))),
        }
        Ok(())
    }

    /// デコードしたフレームを変換して `out` にコピーする。`end` に届いたら true。
    fn accept(
        &mut self,
        decoded: &mut frame::Audio,
        first: i64,
        end: i64,
        out: &mut [f32],
    ) -> Result<bool> {
        let start = match (&self.chunk, decoded.pts()) {
            (Some(prev), _) => prev.end(),
            (None, Some(p)) => self.tb.samples_round(p, DEFAULT_SAMPLE_RATE) - self.origin_samples,
            (None, None) => return Ok(false),
        };
        let samples = self.resample_packed(decoded)?;
        let chunk = Chunk { start, samples };
        copy_overlap(
            &chunk.samples,
            chunk.start,
            first,
            end,
            usize::from(DEFAULT_CHANNELS),
            out,
        );
        let reached = chunk.end() >= end;
        self.chunk = Some(chunk);
        Ok(reached)
    }

    /// 直前の区間の続きとして、デコーダーの今の位置から読み進めてよいか。
    fn continues_at(&self, first: i64) -> bool {
        match &self.chunk {
            Some(c) if self.eof => first >= c.start,
            Some(c) => first >= c.start && first <= c.end() + FORWARD_SAMPLES,
            None => false,
        }
    }

    /// `first` サンプルの少し前のパケットから読み直せるようにする。
    fn seek(&mut self, first: i64) -> Result<()> {
        let t = RationalTime::from_frames(first, Rational::new(DEFAULT_SAMPLE_RATE, 1)?)?
            .checked_add(RationalTime::new(
                self.origin_samples as i128,
                DEFAULT_SAMPLE_RATE,
            )?)?
            .checked_sub(RationalTime::seconds(PREROLL_SECONDS))?;
        // 先頭の近くは 0 より前を指す。0 を指すと、前置きがちょうど 1 パケットの AAC で前置きのパケット（PTS が負）を
        // スキップし、デコーダーが最初の 1 パケット分を出さない。先頭より前へ戻れない形式は 0 から読む。
        let ts = self.tb.ticks_floor(t);
        let ret = self.seek_backward(ts);
        let ret = if ret < 0 && ts < 0 {
            self.seek_backward(0)
        } else {
            ret
        };
        if ret < 0 {
            return Err(MmError::Corrupt(format!(
                "音のシーク: {}",
                ff::Error::from(ret)
            )));
        }
        self.decoder.flush();
        self.resampler = None;
        self.chunk = None;
        self.eof = false;
        Ok(())
    }

    /// 音のストリームの `ts`（ストリームの時間の単位）以前のパケットへシークする。FFmpeg の戻り値を返す。
    fn seek_backward(&mut self, ts: i64) -> i32 {
        // SAFETY: `input` は開いたままの文脈で、ストリームの番号は開いたときに確かめてある。
        unsafe {
            ff::ffi::av_seek_frame(
                self.input.as_mut_ptr(),
                self.stream_index as i32,
                ts,
                ff::ffi::AVSEEK_FLAG_BACKWARD,
            )
        }
    }

    /// デコードしたフレームを、48 kHz・ステレオ・チャンネル交互の f32 にそろえる。
    fn resample_packed(&mut self, decoded: &mut frame::Audio) -> Result<Vec<f32>> {
        if decoded.channel_layout().is_empty() {
            decoded.set_channel_layout(ChannelLayout::default(decoded.channels() as i32));
        }
        if self.resampler.is_none() {
            self.resampler = Some(
                Resampler::get(
                    decoded.format(),
                    decoded.channel_layout(),
                    decoded.rate(),
                    Sample::F32(Type::Packed),
                    ChannelLayout::STEREO,
                    DEFAULT_SAMPLE_RATE,
                )
                .map_err(|e| MmError::Unsupported(format!("音の形式の変換: {e}")))?,
            );
        }
        // 出力バッファが小さいと変換し切れず、残りが内部に溜まって位置が遅れ続ける
        let capacity = decoded.samples() * DEFAULT_SAMPLE_RATE as usize
            / decoded.rate().max(1) as usize
            + OUTPUT_SLACK;
        let mut converted =
            frame::Audio::new(Sample::F32(Type::Packed), capacity, ChannelLayout::STEREO);
        self.resampler
            .as_mut()
            .expect("直前に作った")
            .run(decoded, &mut converted)
            .map_err(|e| MmError::Corrupt(format!("音の変換: {e}")))?;
        let len = converted.samples() * usize::from(DEFAULT_CHANNELS);
        Ok(converted.plane::<f32>(0)[..len].to_vec())
    }
}

/// 秒を出力のサンプル数に丸める。
fn round_to_samples(t: RationalTime) -> Result<i64> {
    let scaled = t.scale(Rational::new(DEFAULT_SAMPLE_RATE, 1)?)?;
    Ok((scaled.num() * 2 + scaled.den() as i128).div_euclid(scaled.den() as i128 * 2) as i64)
}

/// フレーム（`start` から並ぶ）のうち、`[first, end)` に入る分を `out` にコピーする。
fn copy_overlap(
    packed: &[f32],
    start: i64,
    first: i64,
    end: i64,
    channels: usize,
    out: &mut [f32],
) {
    let frames = (packed.len() / channels) as i64;
    let from = first.max(start);
    let to = end.min(start + frames);
    if to <= from {
        return;
    }
    let src = &packed[((from - start) as usize) * channels..((to - start) as usize) * channels];
    let dst = &mut out[((from - first) as usize) * channels..((to - first) as usize) * channels];
    dst.copy_from_slice(src);
}
