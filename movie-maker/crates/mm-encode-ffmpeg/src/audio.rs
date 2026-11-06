//! 音のトラック: f32 のインターリーブ → プレーナー → AAC。

use ff::format::context::Output;
use ff::format::sample::{Sample, Type};
use ff::{codec, encoder, frame, ChannelLayout, Rational};
use ffmpeg_next as ff;
use mm_audio_api::AudioBlock;
use mm_decode_api::AudioStreamInfo;
use mm_types::{MmError, Result};

/// AAC のビットレート（bit/s）。
const BIT_RATE: usize = 192_000;

/// 音のエンコーダーと、出力の中のストリーム。
pub struct AudioTrack {
    encoder: codec::encoder::audio::Encoder,
    stream_index: usize,
    time_base: Rational,
    stream_tb: Rational,
    rate: u32,
    channels: u16,
    /// AAC の 1 フレームに満たない端数のサンプル（インターリーブ）。
    pending: Vec<f32>,
    /// エンコーダーに渡したサンプル数。次のフレームの PTS。
    sent: i64,
}

impl AudioTrack {
    /// 出力にストリームを足し、AAC のエンコーダーを開く。
    pub fn open(output: &mut Output, info: &AudioStreamInfo, global_header: bool) -> Result<Self> {
        let codec = encoder::find(codec::Id::AAC)
            .ok_or_else(|| MmError::Unsupported("AAC のエンコーダーが無い FFmpeg".into()))?;
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
        enc.set_format(Sample::F32(Type::Planar));
        enc.set_bit_rate(BIT_RATE);
        enc.set_time_base(time_base);
        if global_header {
            enc.set_flags(codec::Flags::GLOBAL_HEADER);
        }
        let opened = enc.open().map_err(|e| {
            MmError::Unsupported(format!(
                "AAC を開けない（{}Hz {}ch）: {e}",
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
            pending: Vec::new(),
            sent: 0,
        })
    }

    /// ヘッダーを書いた後の、muxer が決めたストリームの時間の単位を控える。
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
        let frame_len = self.encoder.frame_size() as usize * self.channels as usize;
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
            Sample::F32(Type::Planar),
            samples,
            ChannelLayout::default(i32::from(self.channels)),
        );
        frame.set_rate(self.rate);
        for ch in 0..channels {
            let plane = frame.plane_mut::<f32>(ch);
            for (i, out) in plane.iter_mut().take(samples).enumerate() {
                *out = self.pending[from + i * channels + ch];
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
        crate::drain_packets(
            &mut self.encoder,
            output,
            self.stream_index,
            self.time_base,
            self.stream_tb,
            0,
        )
    }
}
