//! 音のトラック: f32 のインターリーブ → AAC（プレーナー）または 24 ビット PCM（インターリーブ）。

use ff::format::context::Output;
use ff::format::sample::{Sample, Type};
use ff::{codec, encoder, frame, ChannelLayout, Rational};
use ffmpeg_next as ff;
use mm_audio_api::AudioBlock;
use mm_decode_api::AudioStreamInfo;
use mm_types::{MmError, Result};

use crate::spec::AudioCodec;

/// AAC のビットレート（bit/s）。
const BIT_RATE: usize = 192_000;
/// フレームの大きさが決まっていない PCM を送る 1 回のサンプル数（チャンネルあたり）。
const PCM_CHUNK: usize = 4096;
/// 24 ビットの PCM は 32 ビット整数の上位 24 ビットで渡す。f32 の ±1.0 をこの値に対応させる。
const I32_SCALE: f32 = 2_147_483_647.0;

/// 音のエンコーダーと、出力の中のストリーム。
pub struct AudioTrack {
    encoder: codec::encoder::audio::Encoder,
    stream_index: usize,
    time_base: Rational,
    stream_tb: Rational,
    rate: u32,
    channels: u16,
    codec_kind: AudioCodec,
    /// AAC の 1 フレームに満たない端数のサンプル（インターリーブ）。
    pending: Vec<f32>,
    /// エンコーダーに渡したサンプル数。次のフレームの PTS。
    sent: i64,
}

impl AudioTrack {
    /// 出力にストリームを足し、音のエンコーダーを開く。
    pub fn open(
        output: &mut Output,
        info: &AudioStreamInfo,
        kind: AudioCodec,
        global_header: bool,
    ) -> Result<Self> {
        let (codec_id, label) = match kind {
            AudioCodec::Aac => (codec::Id::AAC, "AAC"),
            AudioCodec::PcmS24 => (codec::Id::PCM_S24LE, "PCM 24 ビット"),
        };
        let codec = encoder::find(codec_id)
            .ok_or_else(|| MmError::Unsupported(format!("{label} のエンコーダーが無い FFmpeg")))?;
        let mut stream = output
            .add_stream(codec)
            .map_err(|e| MmError::Internal(format!("音ストリーム: {e}")))?;
        let stream_index = stream.index();
        let mut enc = codec::context::Context::new_with_codec(codec)
            .encoder()
            .audio()
            .map_err(|e| MmError::Internal(format!("音のエンコーダー: {e}")))?;
        let time_base = Rational::new(1, info.sample_rate as i32);
        enc.set_rate(info.sample_rate as i32);
        enc.set_channel_layout(ChannelLayout::default(i32::from(info.channels)));
        enc.set_format(sample_format(kind));
        if kind == AudioCodec::Aac {
            enc.set_bit_rate(BIT_RATE);
        }
        enc.set_time_base(time_base);
        if global_header {
            enc.set_flags(codec::Flags::GLOBAL_HEADER);
        }
        let opened = enc.open_with(encoder_options(kind)).map_err(|e| {
            MmError::Unsupported(format!(
                "{label} を開けない（{}Hz {}ch）: {e}",
                info.sample_rate, info.channels
            ))
        })?;
        stream.set_parameters(&opened);
        stream.set_time_base(time_base);
        Ok(Self {
            encoder: opened,
            stream_index,
            time_base,
            stream_tb: time_base,
            rate: info.sample_rate,
            channels: info.channels,
            codec_kind: kind,
            pending: Vec::new(),
            sent: 0,
        })
    }

    /// ヘッダーを書いた後の、muxer が決めたストリームの時間の単位を記録する。
    pub fn after_header(mut self, output: &Output) -> Self {
        if let Some(stream) = output.stream(self.stream_index) {
            self.stream_tb = stream.time_base();
        }
        self
    }

    /// 音を溜め、AAC の 1 フレーム分たまるごとにエンコードする。
    pub fn push(&mut self, output: &mut Output, block: &AudioBlock) -> Result<()> {
        if block.sample_rate != self.rate || block.channels != self.channels {
            return Err(MmError::InvalidArgument(format!(
                "音の形式が設定と違う: {}Hz {}ch（設定は {}Hz {}ch）",
                block.sample_rate, block.channels, self.rate, self.channels
            )));
        }
        self.pending.extend_from_slice(&block.samples);
        let per_channel = match self.encoder.frame_size() as usize {
            0 => PCM_CHUNK,
            n => n,
        };
        let frame_len = per_channel * self.channels as usize;
        let mut taken = 0;
        while self.pending.len() - taken >= frame_len {
            self.send(output, taken, frame_len)?;
            taken += frame_len;
        }
        self.pending.drain(..taken);
        Ok(())
    }

    /// 端数を短い最後のフレームとして送り、エンコーダーを出し切る。
    pub fn finish(&mut self, output: &mut Output) -> Result<()> {
        if !self.pending.is_empty() {
            let len = self.pending.len();
            self.send(output, 0, len)?;
            self.pending.clear();
        }
        self.encoder
            .send_eof()
            .map_err(|e| MmError::Internal(format!("音の終端: {e}")))?;
        self.drain(output)
    }

    /// `pending[from..from + len]` を 1 フレームにしてエンコードする。
    fn send(&mut self, output: &mut Output, from: usize, len: usize) -> Result<()> {
        let channels = self.channels as usize;
        let samples = len / channels;
        let mut frame = frame::Audio::new(
            sample_format(self.codec_kind),
            samples,
            ChannelLayout::default(i32::from(self.channels)),
        );
        frame.set_rate(self.rate);
        let src = &self.pending[from..from + len];
        match self.codec_kind {
            AudioCodec::Aac => {
                for ch in 0..channels {
                    let plane = frame.plane_mut::<f32>(ch);
                    for (out, frame_samples) in plane.iter_mut().zip(src.chunks_exact(channels)) {
                        *out = frame_samples[ch];
                    }
                }
            }
            AudioCodec::PcmS24 => {
                let plane = frame.plane_mut::<i32>(0);
                for (out, sample) in plane.iter_mut().zip(src) {
                    // 24 ビットの PCM エンコーダーは上位 24 ビットだけを使う
                    *out = (sample.clamp(-1.0, 1.0) * I32_SCALE) as i32;
                }
            }
        }
        frame.set_pts(Some(self.sent));
        self.sent += samples as i64;
        self.encoder
            .send_frame(&frame)
            .map_err(|e| MmError::Internal(format!("音の送信: {e}")))?;
        self.drain(output)
    }

    fn drain(&mut self, output: &mut Output) -> Result<()> {
        crate::writer::drain_packets(
            &mut self.encoder,
            Some(output),
            self.stream_index,
            self.time_base,
            self.stream_tb,
            0,
        )
    }
}

fn sample_format(kind: AudioCodec) -> Sample {
    match kind {
        AudioCodec::Aac => Sample::F32(Type::Planar),
        AudioCodec::PcmS24 => Sample::I32(Type::Packed),
    }
}

/// エンコーダーを開くときの設定。FFmpeg の AAC エンコーダーの雑音の置き換え（PNS）は、
/// 静かな所に数サンプルで 0 dBFS を超えるクリックを作ることがあるので切る。
fn encoder_options(kind: AudioCodec) -> ff::Dictionary<'static> {
    let mut options = ff::Dictionary::new();
    if kind == AudioCodec::Aac {
        options.set("aac_pns", "0");
    }
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 開いたエンコーダーの非公開の設定の整数値。
    fn private_int(track: &AudioTrack, name: &std::ffi::CStr) -> i64 {
        let mut value = -1i64;
        // SAFETY: 開いたエンコーダーの文脈を読むだけ。AV_OPT_SEARCH_CHILDREN でコーデックの非公開の設定を引く。
        let ret = unsafe {
            ff::ffi::av_opt_get_int(
                track.encoder.as_ptr() as *mut std::ffi::c_void,
                name.as_ptr(),
                ff::ffi::AV_OPT_SEARCH_CHILDREN,
                &mut value,
            )
        };
        assert!(ret >= 0, "設定 {name:?} を読めない: {ret}");
        value
    }

    #[test]
    fn aac_は雑音の置き換えを切って開く() {
        ff::init().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut output = ff::format::output(&dir.path().join("a.m4a")).unwrap();
        let info = AudioStreamInfo {
            sample_rate: 48_000,
            channels: 2,
        };
        let track = AudioTrack::open(&mut output, &info, AudioCodec::Aac, true).unwrap();
        assert_eq!(private_int(&track, c"aac_pns"), 0);
    }
}
