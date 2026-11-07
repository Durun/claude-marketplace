//! 映像の無い素材（WAV、FLAC、M4A）。フレームを持たず、音だけを返す。
//!
//! 素材の時間軸の 0 は音の最初のサンプル。映像の無い素材では、映像の最初のフレームに当たる基準が無いため。

use std::path::{Path, PathBuf};

use ffmpeg_next as ff;
use mm_audio_api::{AudioBlock, DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};
use mm_decode_api::{AudioStreamInfo, DecodedFrame, Decoder, MediaInfo};
use mm_types::{ColorSpace, MmError, RationalTime, Result, Size, TimeRange};

use crate::audio::{self, AudioProbe, AudioReader};

/// コンテナの長さ（`AV_TIME_BASE` の単位）を秒にする分母。
const AV_TIME_BASE: u32 = 1_000_000;

/// 映像の無い素材 1 本。
pub struct SoundOnlyDecoder {
    path: PathBuf,
    info: MediaInfo,
    probe: AudioProbe,
    audio: Option<AudioReader>,
}

impl SoundOnlyDecoder {
    /// 素材に映像が無く音があれば開く。映像があれば `None`（映像のデコーダーで開く）。
    pub fn open_if_sound_only(path: &Path) -> Result<Option<Self>> {
        if !path.exists() {
            return Err(MmError::NotFound(path.display().to_string()));
        }
        let input = ff::format::input(&path)
            .map_err(|e| MmError::Corrupt(format!("{}: {e}", path.display())))?;
        if input.streams().best(ff::media::Type::Video).is_some() {
            return Ok(None);
        }
        let Some(probe) = audio::probe(&input)? else {
            return Err(MmError::Unsupported(format!(
                "{}: 映像も音も無い",
                path.display()
            )));
        };
        let duration = RationalTime::new(i128::from(input.duration().max(0)), AV_TIME_BASE)?;
        let info = MediaInfo {
            duration,
            fps: None,
            size: Size::new(0, 0),
            color_space: ColorSpace::bt709(),
            timebase_offset: RationalTime::ZERO,
            audio: Some(AudioStreamInfo {
                sample_rate: DEFAULT_SAMPLE_RATE,
                channels: DEFAULT_CHANNELS,
            }),
            pts_index: Vec::new(),
        };
        Ok(Some(Self {
            path: path.to_owned(),
            info,
            probe,
            audio: None,
        }))
    }
}

impl Decoder for SoundOnlyDecoder {
    fn backend_id(&self) -> Option<&'static str> {
        Some(crate::ID)
    }

    fn info(&self) -> &MediaInfo {
        &self.info
    }

    fn seek(&mut self, _t: RationalTime) -> Result<()> {
        Ok(())
    }

    fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        Ok(None)
    }

    fn decode_audio(&mut self, range: TimeRange) -> Result<AudioBlock> {
        if self.audio.is_none() {
            self.audio = Some(AudioReader::open(&self.path, self.probe, self.probe.start)?);
        }
        self.audio.as_mut().expect("直前に開いた").read(range)
    }
}
