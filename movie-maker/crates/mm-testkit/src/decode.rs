//! デコードとエンコードの見本と契約テスト。
//!
//! 見本のデコーダーは単色のコマを返し、コマの番号を G の成分に `index / FRAME_INDEX_SCALE` で
//! 焼き込む。契約テストとの縦の 1 本は、この値を読んで「どのコマが出たか」を確かめる。

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mm_audio_api::{AudioBlock, DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};
use mm_decode_api::{
    AudioStreamInfo, DecodedFrame, Decoder, DecoderFactory, EncodeReport, EncodeSettings, Encoder,
    EncoderFactory, FrameSource, GpuContextRef, MediaInfo,
};
use mm_types::{
    ColorSpace, MmError, Rational, RationalTime, Result, Rgba, RgbaImage, Size, TimeRange,
};

/// コマの番号を G に焼くときの分母。f32 で整数が正確に表せる範囲（2^24）に収まる。
pub const FRAME_INDEX_SCALE: f32 = 65_536.0;
/// 見本の音のトーンの周波数（Hz）。
pub const TONE_HZ: f32 = 440.0;
/// 見本の音の振幅。
pub const TONE_AMPLITUDE: f32 = 0.1;
/// パスにこの語が入っていれば壊れたファイルとして扱う。
pub const BROKEN_MARKER: &str = "broken";

/// 見本の素材の形。
#[derive(Debug, Clone, PartialEq)]
pub struct SampleMedia {
    pub duration: RationalTime,
    pub fps: Rational,
    pub size: Size,
    pub base_color: Rgba,
    pub with_audio: bool,
}

impl Default for SampleMedia {
    fn default() -> Self {
        Self {
            duration: RationalTime::seconds(10),
            fps: Rational::integer(30),
            size: Size::new(320, 180),
            base_color: Rgba::new(0.25, 0.0, 0.5, 1.0),
            with_audio: true,
        }
    }
}

impl SampleMedia {
    fn info(&self) -> MediaInfo {
        MediaInfo {
            duration: self.duration,
            fps: Some(self.fps),
            size: self.size,
            color_space: ColorSpace::bt709(),
            timebase_offset: RationalTime::ZERO,
            audio: self.with_audio.then_some(AudioStreamInfo {
                sample_rate: DEFAULT_SAMPLE_RATE,
                channels: DEFAULT_CHANNELS,
            }),
            pts_index: Vec::new(),
        }
    }
}

/// コマの番号を焼き込んだ画素の色。
pub fn frame_color(base: Rgba, index: i64) -> Rgba {
    Rgba::new(base.r, index as f32 / FRAME_INDEX_SCALE, base.b, base.a)
}

/// 画素からコマの番号を読む。
pub fn frame_index_of(pixel: Rgba) -> i64 {
    (pixel.g * FRAME_INDEX_SCALE).round() as i64
}

/// 見本のデコーダーを開く口。パスは無視し、`BROKEN_MARKER` を含むパスだけ `Corrupt` を返す。
#[derive(Debug, Clone, Default)]
pub struct SampleDecoderFactory {
    pub media: SampleMedia,
}

impl DecoderFactory for SampleDecoderFactory {
    fn id(&self) -> &'static str {
        crate::SAMPLE_ID
    }

    fn open(&self, path: &Path, _gpu: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>> {
        if path.to_string_lossy().contains(BROKEN_MARKER) {
            return Err(MmError::Corrupt(format!("{}", path.display())));
        }
        Ok(Box::new(SampleDecoder {
            media: self.media.clone(),
            info: self.media.info(),
            cursor: 0,
        }))
    }
}

/// 見本のデコーダー。
#[derive(Debug)]
pub struct SampleDecoder {
    media: SampleMedia,
    info: MediaInfo,
    cursor: i64,
}

impl Decoder for SampleDecoder {
    fn info(&self) -> &MediaInfo {
        &self.info
    }

    fn seek(&mut self, t: RationalTime) -> Result<()> {
        self.cursor = t.frame_index(self.media.fps);
        Ok(())
    }

    fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        if self.cursor < 0 || self.cursor >= self.info.frame_count() {
            return Ok(None);
        }
        let index = self.cursor;
        self.cursor += 1;
        let image = RgbaImage::filled(self.media.size, frame_color(self.media.base_color, index));
        Ok(Some(DecodedFrame::cpu(
            index,
            RationalTime::from_frames(index, self.media.fps)?,
            image,
        )))
    }

    fn decode_audio(&mut self, range: TimeRange) -> Result<AudioBlock> {
        let rate = DEFAULT_SAMPLE_RATE;
        let first = range.start.frame_index(Rational::integer(rate));
        let frames = mm_node_api::audio_frames_in(range, rate);
        let mut samples = Vec::with_capacity(frames * DEFAULT_CHANNELS as usize);
        for i in 0..frames {
            let n = (first + i as i64) as f32;
            let v = if self.media.with_audio {
                TONE_AMPLITUDE * (TAU * TONE_HZ * n / rate as f32).sin()
            } else {
                0.0
            };
            samples.extend(std::iter::repeat_n(v, DEFAULT_CHANNELS as usize));
        }
        AudioBlock::from_samples(rate, DEFAULT_CHANNELS, samples)
    }
}

/// `Decoder` を時刻で引ける `FrameSource` にする。直前のコマを覚え、同じコマなら復号しない。
pub struct DecoderFrameSource {
    decoder: Box<dyn Decoder>,
    last: Option<DecodedFrame>,
}

impl DecoderFrameSource {
    /// デコーダーを包む。
    pub fn new(decoder: Box<dyn Decoder>) -> Self {
        Self {
            decoder,
            last: None,
        }
    }
}

impl FrameSource for DecoderFrameSource {
    fn info(&self) -> &MediaInfo {
        self.decoder.info()
    }

    fn frame_at(&mut self, t: RationalTime) -> Result<DecodedFrame> {
        let index = self.decoder.info().frame_index_at(t);
        if let Some(last) = &self.last {
            if last.index == index {
                return Ok(last.clone());
            }
        }
        self.decoder.seek(t)?;
        let frame = self
            .decoder
            .next_frame()?
            .ok_or_else(|| MmError::NotFound(format!("コマ {index}（t = {t}）")))?;
        self.last = Some(frame.clone());
        Ok(frame)
    }

    fn audio_in(&mut self, range: TimeRange) -> Result<AudioBlock> {
        self.decoder.decode_audio(range)
    }
}

/// 見本のエンコーダーを作る口。
#[derive(Debug, Clone, Default)]
pub struct SampleEncoderFactory;

impl EncoderFactory for SampleEncoderFactory {
    fn id(&self) -> &'static str {
        crate::SAMPLE_ID
    }

    fn supports(&self, codec: &str) -> bool {
        codec == crate::SAMPLE_ID
    }

    fn create(&self) -> Result<Box<dyn Encoder>> {
        Ok(Box::new(SampleEncoder::default()))
    }
}

/// 見本のエンコーダー。コマと音を数え、`output_path` に JSON の報告を書く。
#[derive(Debug, Default)]
pub struct SampleEncoder {
    settings: Option<EncodeSettings>,
    frames: u64,
    audio_frames: u64,
    frame_indices: Vec<i64>,
}

impl SampleEncoder {
    /// 押されたコマから読んだ番号の列。縦の 1 本のテストが見る。
    pub fn frame_indices(&self) -> &[i64] {
        &self.frame_indices
    }
}

impl Encoder for SampleEncoder {
    fn begin(&mut self, settings: &EncodeSettings) -> Result<()> {
        self.settings = Some(settings.clone());
        Ok(())
    }

    fn push_frames(&mut self, frames: &[RgbaImage]) -> Result<()> {
        let settings = self
            .settings
            .as_ref()
            .ok_or_else(|| MmError::InvalidArgument("begin の前に push_frames".into()))?;
        for f in frames {
            if f.size() != settings.size {
                return Err(MmError::InvalidArgument("コマの大きさが設定と違う".into()));
            }
            self.frame_indices.push(frame_index_of(f.pixel(0, 0)));
        }
        self.frames += frames.len() as u64;
        Ok(())
    }

    fn push_audio(&mut self, block: &AudioBlock) -> Result<()> {
        self.audio_frames += block.frames() as u64;
        Ok(())
    }

    fn finish(&mut self) -> Result<EncodeReport> {
        let settings = self
            .settings
            .take()
            .ok_or_else(|| MmError::InvalidArgument("begin の前に finish".into()))?;
        let report = EncodeReport {
            frames: self.frames,
            audio_frames: self.audio_frames,
            bytes_written: 0,
        };
        let json = serde_json::to_vec_pretty(&report)?;
        std::fs::write(&settings.output_path, &json)?;
        Ok(EncodeReport {
            bytes_written: json.len() as u64,
            ..report
        })
    }
}

/// 契約テストに渡す素材の情報。
#[derive(Debug, Clone)]
pub struct DecodeFixture {
    pub path: PathBuf,
    pub duration: RationalTime,
    pub fps: Rational,
    /// 壊れたファイル。`None` なら壊れた入力の検査を飛ばす。
    pub broken_path: Option<PathBuf>,
}

impl DecodeFixture {
    /// 見本のデコーダー向け。
    pub fn sample(media: &SampleMedia) -> Self {
        Self {
            path: PathBuf::from("sample.mp4"),
            duration: media.duration,
            fps: media.fps,
            broken_path: Some(PathBuf::from("sample.broken.mp4")),
        }
    }
}

/// デコーダーの契約。`make` は毎回新しい口を作る。
pub fn contract(make: impl Fn() -> Arc<dyn DecoderFactory>, fx: &DecodeFixture) {
    let factory = make();
    let mut dec = factory.open(&fx.path, None).expect("開ける");
    assert_eq!(dec.info().duration, fx.duration, "duration が合う");
    assert_eq!(dec.info().fps, Some(fx.fps), "fps が合う");

    let count = dec.info().frame_count();
    let mut n = 0;
    while dec.next_frame().expect("復号").is_some() {
        n += 1;
    }
    assert_eq!(n, count, "コマの数が duration と fps から数えた数と合う");

    let t = RationalTime::from_frames(count / 2, fx.fps).expect("表せる");
    dec.seek(t).expect("シーク");
    let f1 = dec.next_frame().expect("復号").expect("コマがある");
    assert_eq!(f1.index, t.frame_index(fx.fps), "シーク先のコマの番号");
    assert_eq!(f1.pts, t, "シーク先の PTS");
    dec.seek(t).expect("シーク");
    let f2 = dec.next_frame().expect("復号").expect("コマがある");
    assert_eq!(f1, f2, "同じ時刻にシークし直すと同じコマ");

    let end = RationalTime::from_frames(5, fx.fps).expect("表せる");
    let range = TimeRange::new(RationalTime::ZERO, end).unwrap();
    let frames = dec.decode_range(range).expect("区間");
    let indices: Vec<i64> = frames.iter().map(|f| f.index).collect();
    assert_eq!(indices, range.frame_indices(fx.fps).collect::<Vec<_>>());
    assert_eq!(
        indices,
        dec.info().frame_range(range).collect::<Vec<_>>(),
        "口の frame_range と同じ番号"
    );

    if let Some(broken) = &fx.broken_path {
        assert!(
            matches!(make().open(broken, None), Err(MmError::Corrupt(_))),
            "壊れたファイルは Corrupt"
        );
    }
}

/// エンコーダーの契約。`output_dir` に書く。
pub fn encoder_contract(
    make: impl Fn() -> Arc<dyn EncoderFactory>,
    codec: &str,
    output_dir: &Path,
) {
    let factory = make();
    assert!(factory.supports(codec), "{codec} を扱える");
    let mut enc = factory.create().expect("作れる");
    let size = Size::new(16, 8);
    let settings = EncodeSettings {
        size,
        fps: Rational::integer(30),
        codec: codec.to_owned(),
        output_path: output_dir.join(format!("encoder_contract_{codec}.out")),
        audio: Some(AudioStreamInfo {
            sample_rate: DEFAULT_SAMPLE_RATE,
            channels: DEFAULT_CHANNELS,
        }),
    };
    assert!(enc.push_frames(&[]).is_err(), "begin の前は断る");
    enc.begin(&settings).expect("begin");
    let frames: Vec<RgbaImage> = (0..6)
        .map(|i| RgbaImage::filled(size, frame_color(Rgba::BLACK, i)))
        .collect();
    enc.push_frames(&frames[..4]).expect("push");
    enc.push_frames(&frames[4..]).expect("push");
    enc.push_audio(&AudioBlock::silence(
        DEFAULT_SAMPLE_RATE,
        DEFAULT_CHANNELS,
        9_600,
    ))
    .expect("audio");
    let report = enc.finish().expect("finish");
    assert_eq!(report.frames, 6, "押したコマの数");
    assert_eq!(report.audio_frames, 9_600, "押した音のコマの数");
    assert!(settings.output_path.exists(), "出力ができている");
}
