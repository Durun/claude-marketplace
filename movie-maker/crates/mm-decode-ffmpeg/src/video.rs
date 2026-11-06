//! 映像のデコーダー本体。時間軸、番号づけ、シークは `mm-decode-ffmpeg-core` の共有部が受け持つ。

use std::path::{Path, PathBuf};

use ffmpeg_next as ff;
use mm_audio_api::{AudioBlock, DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};
use mm_decode_api::{AudioStreamInfo, DecodedFrame, Decoder, MediaInfo};
use mm_decode_ffmpeg_core::reader::{IndexedFrame, Sequencer, StreamReader};
use mm_types::{MmError, Rational, RationalTime, Result, TimeRange};

use crate::audio::{self, AudioProbe, AudioReader};
use crate::convert::RgbaConverter;

/// FFmpeg で開いた素材 1 本。
pub struct FfmpegDecoder {
    path: PathBuf,
    seq: Sequencer,
    converter: RgbaConverter,
    audio_probe: Option<AudioProbe>,
    audio: Option<AudioReader>,
}

impl FfmpegDecoder {
    /// 素材を開く。ファイルが無ければ `NotFound`、読めなければ `Corrupt`、PQ と HLG は `Unsupported`。
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
        let mut ctx = ff::codec::Context::from_parameters(stream.parameters())
            .map_err(|e| MmError::Unsupported(format!("映像のコーデック: {e}")))?;
        // 1 スレッドだと 1080p のデコードが数倍遅い。0 は FFmpeg が CPU の数で決める。
        ctx.set_threading(ff::threading::Config {
            kind: ff::threading::Type::Frame,
            ..Default::default()
        });
        let decoder = ctx
            .decoder()
            .video()
            .map_err(|e| MmError::Corrupt(format!("{}: 映像のデコーダー: {e}", path.display())))?;
        let audio_probe = audio::probe(&input)?;
        let mut seq = Sequencer::new(path, StreamReader::new(input, stream_index, decoder))?;
        let timebase_offset = match audio_probe {
            Some(a) => seq.origin().checked_sub(a.start)?,
            None => RationalTime::ZERO,
        };
        // 音は素材の形によらず、読むときに既定のレートとチャンネル数へそろえる
        let audio_info = audio_probe.map(|_| AudioStreamInfo {
            sample_rate: DEFAULT_SAMPLE_RATE,
            channels: DEFAULT_CHANNELS,
        });
        seq.set_audio(audio_info, timebase_offset);
        Ok(Self {
            path: path.to_owned(),
            seq,
            converter: RgbaConverter::default(),
            audio_probe,
            audio: None,
        })
    }
}

impl Decoder for FfmpegDecoder {
    fn info(&self) -> &MediaInfo {
        self.seq.info()
    }

    fn seek(&mut self, t: RationalTime) -> Result<()> {
        self.seq.seek(t)
    }

    fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        let Some(IndexedFrame { index, pts, frame }) = self.seq.next_frame()? else {
            return Ok(None);
        };
        let image = self.converter.convert(&frame)?;
        Ok(Some(DecodedFrame::cpu(index, pts, image)))
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
            self.audio = Some(AudioReader::open(&self.path, probe, self.seq.origin())?);
        }
        self.audio.as_mut().expect("直前に開いた").read(range)
    }
}
