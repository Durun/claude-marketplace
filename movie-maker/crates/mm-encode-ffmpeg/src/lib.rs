//! FFmpeg のライブラリによるエンコーダー（`mm-decode-api` の `Encoder` の実装）。
//!
//! H.264（libx264）+ AAC を MP4 に書く。コマは RGBA の f32 で受け取り、Rec.709・tv レンジの
//! YUV420P にして押す。コマの時刻は 1 コマごとに `1 / fps` 進める（書き出しは CFR）。
//!
//! feature `ffmpeg` を入れたときだけ実装をビルドする（FFmpeg の開発ファイルが要るため）。
#![cfg(feature = "ffmpeg")]

mod audio;
mod video;

use ff::format::context::Output;
use ff::{codec, encoder, format, Packet, Rational};
use ffmpeg_next as ff;
use mm_audio_api::AudioBlock;
use mm_decode_api::{EncodeReport, EncodeSettings, Encoder, EncoderFactory};
use mm_types::{MmError, Result, RgbaImage};

use audio::AudioTrack;
use video::VideoTrack;

/// 実装の名前。
pub const ID: &str = "ffmpeg";

/// `EncodeSettings::codec` に書ける H.264 の名前。
pub const H264_NAMES: [&str; 2] = ["h264", "libx264"];

/// FFmpeg のエンコーダーを作る口。
#[derive(Debug, Clone, Copy, Default)]
pub struct FfmpegEncoderFactory;

impl EncoderFactory for FfmpegEncoderFactory {
    fn id(&self) -> &'static str {
        ID
    }

    fn supports(&self, codec: &str) -> bool {
        H264_NAMES.contains(&codec) && init().is_ok() && encoder::find_by_name("libx264").is_some()
    }

    fn create(&self) -> Result<Box<dyn Encoder>> {
        init()?;
        Ok(Box::new(FfmpegEncoder { session: None }))
    }
}

fn init() -> Result<()> {
    ff::init().map_err(|e| MmError::Internal(format!("FFmpeg の初期化: {e}")))?;
    ff::util::log::set_level(ff::util::log::Level::Error);
    Ok(())
}

/// 書き出し 1 本。`begin` から `finish` まで。
pub struct FfmpegEncoder {
    session: Option<Session>,
}

/// 開いた出力と 2 本のトラック。
struct Session {
    output: Output,
    video: VideoTrack,
    audio: Option<AudioTrack>,
    settings: EncodeSettings,
    frames: u64,
    audio_frames: u64,
}

// FFmpeg の文脈は生ポインタを持つため自動では Send にならない。Session は 1 つの
// FfmpegEncoder だけが所有し、文脈はスレッドに紐づかないので、別スレッドへ移してよい。
unsafe impl Send for Session {}

impl Encoder for FfmpegEncoder {
    fn begin(&mut self, settings: &EncodeSettings) -> Result<()> {
        if self.session.is_some() {
            return Err(MmError::InvalidArgument("begin を 2 回呼んだ".into()));
        }
        if !FfmpegEncoderFactory.supports(&settings.codec) {
            return Err(MmError::Unsupported(format!(
                "コーデック {} は mm-encode-ffmpeg が扱えない（{:?}）",
                settings.codec, H264_NAMES
            )));
        }
        // 拡張子に依らず MP4 として書く（契約テストの出力は .out）
        let mut output = format::output_as(&settings.output_path, "mp4")
            .map_err(|e| MmError::Io(std::io::Error::other(format!("出力を開けない: {e}"))))?;
        let global_header = output
            .format()
            .flags()
            .contains(format::Flags::GLOBAL_HEADER);
        let video = VideoTrack::open(&mut output, settings, global_header)?;
        let audio = settings
            .audio
            .as_ref()
            .map(|a| AudioTrack::open(&mut output, a, global_header))
            .transpose()?;
        let mut options = ff::Dictionary::new();
        options.set("movflags", "+faststart");
        options.set("avoid_negative_ts", "make_non_negative");
        output
            .write_header_with(options)
            .map_err(|e| MmError::Internal(format!("ヘッダーの書き込み: {e}")))?;
        let video = video.after_header(&output);
        let audio = audio.map(|a| a.after_header(&output));
        self.session = Some(Session {
            output,
            video,
            audio,
            settings: settings.clone(),
            frames: 0,
            audio_frames: 0,
        });
        Ok(())
    }

    fn push_frames(&mut self, frames: &[RgbaImage]) -> Result<()> {
        let s = self.session_mut("push_frames")?;
        for image in frames {
            s.video.push(&mut s.output, image)?;
            s.frames += 1;
        }
        Ok(())
    }

    fn push_audio(&mut self, block: &AudioBlock) -> Result<()> {
        let s = self.session_mut("push_audio")?;
        let track = s
            .audio
            .as_mut()
            .ok_or_else(|| MmError::InvalidArgument("設定に音が無いのに push_audio".into()))?;
        track.push(&mut s.output, block)?;
        s.audio_frames += block.frames() as u64;
        Ok(())
    }

    fn finish(&mut self) -> Result<EncodeReport> {
        let mut s = self
            .session
            .take()
            .ok_or_else(|| MmError::InvalidArgument("begin の前に finish".into()))?;
        s.video.finish(&mut s.output)?;
        if let Some(audio) = s.audio.as_mut() {
            audio.finish(&mut s.output)?;
        }
        s.output
            .write_trailer()
            .map_err(|e| MmError::Internal(format!("トレーラーの書き込み: {e}")))?;
        drop(s.output);
        let bytes_written = std::fs::metadata(&s.settings.output_path)?.len();
        Ok(EncodeReport {
            frames: s.frames,
            audio_frames: s.audio_frames,
            bytes_written,
        })
    }
}

impl FfmpegEncoder {
    fn session_mut(&mut self, call: &str) -> Result<&mut Session> {
        self.session
            .as_mut()
            .ok_or_else(|| MmError::InvalidArgument(format!("begin の前に {call}")))
    }
}

/// 符号化器から出たパケットを全部出力に書く。`duration` は符号化器の時間の単位での 1 パケットの長さで、
/// 0 以外なら上書きする（libx264 は入れず、最後のコマの長さが muxer に伝わらないため）。
fn drain_packets(
    encoder: &mut codec::encoder::Encoder,
    output: &mut Output,
    stream_index: usize,
    from: Rational,
    to: Rational,
    duration: i64,
) -> Result<()> {
    let mut packet = Packet::empty();
    loop {
        match encoder.receive_packet(&mut packet) {
            Ok(()) => {
                packet.set_stream(stream_index);
                if duration > 0 {
                    packet.set_duration(duration);
                }
                packet.rescale_ts(from, to);
                packet
                    .write_interleaved(output)
                    .map_err(|e| MmError::Internal(format!("パケットの書き込み: {e}")))?;
            }
            Err(ff::Error::Other { errno }) if errno == ff::error::EAGAIN => return Ok(()),
            Err(ff::Error::Eof) => return Ok(()),
            Err(e) => return Err(MmError::Internal(format!("符号化: {e}"))),
        }
    }
}
