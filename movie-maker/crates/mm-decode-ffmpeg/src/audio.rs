//! 音の復号。映像とは別に素材を開き直し、映像のシークと干渉しないようにする。

use std::path::Path;

use ff::format::sample::{Sample, Type};
use ff::software::resampling::Context as Resampler;
use ff::{frame, ChannelLayout, Packet};
use ffmpeg_next as ff;
use mm_audio_api::AudioBlock;
use mm_types::{MmError, Rational, RationalTime, Result, TimeRange};

use crate::timeline::TimeBase;

/// 区間の頭より前から復号を始める長さ（秒）。AAC などの前の窓の分を捨てるため。
const PREROLL_SECONDS: i64 = 1;

/// 音のストリームを開いた状態。
pub struct AudioReader {
    input: ff::format::context::Input,
    stream_index: usize,
    decoder: ff::decoder::Audio,
    tb: TimeBase,
    rate: u32,
    channels: u16,
    /// 素材の時間軸の 0（映像の最初のコマ）を、音のストリームの ticks で表した値。
    origin: RationalTime,
}

/// 音のストリームの性質。
#[derive(Debug, Clone, Copy)]
pub struct AudioProbe {
    pub stream_index: usize,
    pub rate: u32,
    pub channels: u16,
    /// 音のストリームの最初の PTS（秒）。
    pub start: RationalTime,
}

/// 音のストリームがあれば性質を返す。
pub fn probe(input: &ff::format::context::Input) -> Result<Option<AudioProbe>> {
    let Some(stream) = input.streams().best(ff::media::Type::Audio) else {
        return Ok(None);
    };
    let tb = TimeBase::new(stream.time_base())?;
    let ctx = ff::codec::Context::from_parameters(stream.parameters())
        .map_err(|e| MmError::Unsupported(format!("音のコーデック: {e}")))?;
    let audio = ctx
        .decoder()
        .audio()
        .map_err(|e| MmError::Unsupported(format!("音のデコーダー: {e}")))?;
    let start_ticks = stream.start_time();
    let start = if start_ticks == ff::ffi::AV_NOPTS_VALUE {
        RationalTime::ZERO
    } else {
        tb.time(start_ticks)?
    };
    Ok(Some(AudioProbe {
        stream_index: stream.index(),
        rate: audio.rate(),
        channels: audio.channels(),
        start,
    }))
}

impl AudioReader {
    /// 素材を開き直して音のストリームを復号できる状態にする。
    pub fn open(path: &Path, probe: AudioProbe, origin: RationalTime) -> Result<Self> {
        let input = ff::format::input(&path)
            .map_err(|e| MmError::Corrupt(format!("{}: {e}", path.display())))?;
        let stream = input
            .stream(probe.stream_index)
            .ok_or_else(|| MmError::Corrupt("音のストリームが消えた".into()))?;
        let tb = TimeBase::new(stream.time_base())?;
        let ctx = ff::codec::Context::from_parameters(stream.parameters())
            .map_err(|e| MmError::Unsupported(format!("音のコーデック: {e}")))?;
        let decoder = ctx
            .decoder()
            .audio()
            .map_err(|e| MmError::Unsupported(format!("音のデコーダー: {e}")))?;
        Ok(Self {
            input,
            stream_index: probe.stream_index,
            decoder,
            tb,
            rate: probe.rate,
            channels: probe.channels,
            origin,
        })
    }

    /// 素材の時間軸の区間の音。外は無音。
    pub fn read(&mut self, range: TimeRange) -> Result<AudioBlock> {
        let rate = Rational::new(self.rate, 1)?;
        let first = range.start.frame_index(rate);
        let count = (range.end.frame_index(rate) - first).max(0) as usize;
        let channels = self.channels as usize;
        let mut out = vec![0.0f32; count * channels];
        if count > 0 {
            self.fill(first, &mut out)?;
        }
        AudioBlock::from_samples(self.rate, self.channels, out)
    }

    fn fill(&mut self, first: i64, out: &mut [f32]) -> Result<()> {
        let channels = self.channels as usize;
        let end = first + (out.len() / channels) as i64;
        self.seek(first)?;
        let mut resampler: Option<Resampler> = None;
        let mut packet = Packet::empty();
        let mut draining = false;
        let origin_samples = round_to_samples(self.origin, self.rate)?;
        loop {
            let mut decoded = frame::Audio::empty();
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    let start = match decoded.pts() {
                        Some(p) => self.tb.samples_round(p, self.rate) - origin_samples,
                        None => continue,
                    };
                    let len = decoded.samples() as i64;
                    if start + len <= first {
                        continue;
                    }
                    if start >= end {
                        return Ok(());
                    }
                    let packed = self.to_packed(&mut resampler, &mut decoded)?;
                    copy_overlap(&packed, start, first, end, channels, out);
                    continue;
                }
                Err(ff::Error::Eof) => return Ok(()),
                Err(ff::Error::Other { errno }) if errno == ff::error::EAGAIN => {}
                Err(e) => return Err(MmError::Corrupt(format!("音の復号: {e}"))),
            }
            if draining {
                return Ok(());
            }
            match packet.read(&mut self.input) {
                Ok(()) if packet.stream() == self.stream_index => {
                    match self.decoder.send_packet(&packet) {
                        Ok(()) | Err(ff::Error::InvalidData) => {}
                        Err(e) => return Err(MmError::Corrupt(format!("音のパケット: {e}"))),
                    }
                }
                Ok(()) | Err(ff::Error::InvalidData) => {}
                Err(ff::Error::Eof) => {
                    // 終端の読み残しを出させる
                    let _ = self.decoder.send_eof();
                    draining = true;
                }
                Err(e) => return Err(MmError::Corrupt(format!("音の読み込み: {e}"))),
            }
        }
    }

    /// `first` サンプルの少し前のパケットから読み直せるようにする。
    fn seek(&mut self, first: i64) -> Result<()> {
        let t = RationalTime::from_frames(first, Rational::new(self.rate, 1)?)?
            .checked_add(self.origin)?
            .checked_sub(RationalTime::seconds(PREROLL_SECONDS))?;
        let ts = self.tb.ticks_floor(t.max(RationalTime::ZERO));
        unsafe {
            let ret = ff::ffi::av_seek_frame(
                self.input.as_mut_ptr(),
                self.stream_index as i32,
                ts,
                ff::ffi::AVSEEK_FLAG_BACKWARD,
            );
            if ret < 0 {
                return Err(MmError::Corrupt(format!(
                    "音のシーク: {}",
                    ff::Error::from(ret)
                )));
            }
        }
        self.decoder.flush();
        Ok(())
    }

    /// 復号したフレームを、チャンネル交互の f32 にそろえる。
    fn to_packed(
        &self,
        resampler: &mut Option<Resampler>,
        decoded: &mut frame::Audio,
    ) -> Result<Vec<f32>> {
        if decoded.channel_layout().is_empty() {
            decoded.set_channel_layout(ChannelLayout::default(i32::from(self.channels)));
        }
        if resampler.is_none() {
            *resampler = Some(
                Resampler::get(
                    decoded.format(),
                    decoded.channel_layout(),
                    decoded.rate(),
                    Sample::F32(Type::Packed),
                    decoded.channel_layout(),
                    decoded.rate(),
                )
                .map_err(|e| MmError::Unsupported(format!("音の形式の変換: {e}")))?,
            );
        }
        let mut converted = frame::Audio::empty();
        resampler
            .as_mut()
            .expect("直前に作った")
            .run(decoded, &mut converted)
            .map_err(|e| MmError::Corrupt(format!("音の変換: {e}")))?;
        Ok(converted.plane::<f32>(0).to_vec())
    }
}

fn round_to_samples(t: RationalTime, rate: u32) -> Result<i64> {
    let scaled = t.scale(Rational::new(rate, 1)?)?;
    Ok((scaled.num() * 2 + scaled.den() as i128).div_euclid(scaled.den() as i128 * 2) as i64)
}

/// フレーム（`start` から並ぶ）のうち、`[first, end)` に入る分を `out` に写す。
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
