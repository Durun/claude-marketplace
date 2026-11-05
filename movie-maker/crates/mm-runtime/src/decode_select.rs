//! デコードの選択肢の切り替え。GPU で復号する実装を素材ごとに試し、だめなら次へ落とす。
//!
//! - [`AutoDecoderFactory`]: `d3d12va` → `cuvid` → `ffmpeg` の順に、素材ごとに開く。開いた直後の数コマを
//!   復号して通った実装を使う（NVENC で作った 4K の HEVC は d3d12va が 1 コマ目で落ちる）。
//!   GPU の文脈が無ければ最初から `ffmpeg`。
//! - [`WithAudio`]: 映像を GPU の実装、音と素材の情報（音のストリーム、頭のずれ）を別のデコーダーが受け持つ。
//!   GPU の実装は音を持たないため。

use std::path::Path;
use std::sync::Arc;

use mm_audio_api::AudioBlock;
use mm_decode_api::{DecodedFrame, Decoder, DecoderFactory, GpuContextRef, MediaInfo};
use mm_types::{MmError, RationalTime, Result, TimeRange};

/// 開いた直後に復号して確かめるコマ数。0 コマ目で落ちる素材だけでなく、数コマ後に落ちるものも拾う。
const PROBE_FRAMES: usize = 4;

/// 素材ごとに GPU の実装を試して、通ったものを使う口。
pub struct AutoDecoderFactory {
    /// 試す順に並べた GPU の実装。
    gpu: Vec<Arc<dyn DecoderFactory>>,
    /// 最後の逃げ道（CPU）。
    fallback: Option<Arc<dyn DecoderFactory>>,
}

impl AutoDecoderFactory {
    /// `gpu` を前から順に試し、全部だめなら `fallback`。
    pub fn new(
        gpu: Vec<Arc<dyn DecoderFactory>>,
        fallback: Option<Arc<dyn DecoderFactory>>,
    ) -> Self {
        Self { gpu, fallback }
    }

    /// 素材を開き、どの実装が通ったか（`id`）も返す。
    pub fn open_with_choice(
        &self,
        path: &Path,
        gpu: Option<&dyn GpuContextRef>,
    ) -> Result<(Box<dyn Decoder>, &'static str)> {
        let mut last_error = None;
        for factory in self.gpu.iter().filter(|_| gpu.is_some()) {
            match open_and_probe(factory.as_ref(), path, gpu) {
                Ok(dec) => return Ok((dec, factory.id())),
                Err(e) => {
                    tracing::warn!(backend = factory.id(), path = %path.display(), error = %e, "GPU の復号を使えない。次の実装を試す");
                    last_error = Some(e);
                }
            }
        }
        match &self.fallback {
            Some(f) => Ok((f.open(path, gpu)?, f.id())),
            None => Err(last_error.unwrap_or_else(|| {
                MmError::Unsupported("使えるデコーダーが無い（GPU の文脈か ffmpeg が要る）".into())
            })),
        }
    }
}

impl DecoderFactory for AutoDecoderFactory {
    fn id(&self) -> &'static str {
        "auto"
    }

    fn open(&self, path: &Path, gpu: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>> {
        self.open_with_choice(path, gpu).map(|(dec, _)| dec)
    }
}

/// 開いて先頭の数コマを復号し、通れば最初の位置に戻して返す。
fn open_and_probe(
    factory: &dyn DecoderFactory,
    path: &Path,
    gpu: Option<&dyn GpuContextRef>,
) -> Result<Box<dyn Decoder>> {
    let mut dec = factory.open(path, gpu)?;
    for _ in 0..PROBE_FRAMES {
        if dec.next_frame()?.is_none() {
            break;
        }
    }
    dec.seek(RationalTime::ZERO)?;
    Ok(dec)
}

/// 映像を `video`、音を `audio` が受け持つデコーダー。素材の情報は映像の時間軸に音の情報を足す。
pub struct SplitDecoder {
    video: Box<dyn Decoder>,
    audio: Box<dyn Decoder>,
    info: MediaInfo,
}

impl SplitDecoder {
    /// 映像と音のデコーダーを組にする。
    pub fn new(video: Box<dyn Decoder>, audio: Box<dyn Decoder>) -> Self {
        let mut info = video.info().clone();
        info.audio = audio.info().audio.clone();
        info.timebase_offset = audio.info().timebase_offset;
        Self { video, audio, info }
    }
}

impl Decoder for SplitDecoder {
    fn info(&self) -> &MediaInfo {
        &self.info
    }

    fn seek(&mut self, t: RationalTime) -> Result<()> {
        self.video.seek(t)
    }

    fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        self.video.next_frame()
    }

    fn decode_audio(&mut self, range: TimeRange) -> Result<AudioBlock> {
        self.audio.decode_audio(range)
    }
}

/// 映像の実装に、音を別の実装で付ける口。
pub struct WithAudio {
    video: Arc<dyn DecoderFactory>,
    audio: Arc<dyn DecoderFactory>,
}

impl WithAudio {
    /// `video` の実装に `audio` の音を付ける。
    pub fn new(video: Arc<dyn DecoderFactory>, audio: Arc<dyn DecoderFactory>) -> Self {
        Self { video, audio }
    }
}

impl DecoderFactory for WithAudio {
    fn id(&self) -> &'static str {
        self.video.id()
    }

    fn open(&self, path: &Path, gpu: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>> {
        let video = self.video.open(path, gpu)?;
        let audio = self.audio.open(path, None)?;
        Ok(Box::new(SplitDecoder::new(video, audio)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_testkit::decode::SampleDecoderFactory;

    /// 開けない実装。
    struct CannotOpen;
    impl DecoderFactory for CannotOpen {
        fn id(&self) -> &'static str {
            "cannot_open"
        }
        fn open(&self, _: &Path, _: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>> {
            Err(MmError::Unsupported("開けない".into()))
        }
    }

    /// 開けるが、`fail_at` 番目のコマで落ちる実装。
    struct FailsAt(usize);
    struct FailingDecoder {
        inner: Box<dyn Decoder>,
        fail_at: usize,
        count: usize,
    }
    impl DecoderFactory for FailsAt {
        fn id(&self) -> &'static str {
            "fails_at"
        }
        fn open(&self, p: &Path, g: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>> {
            Ok(Box::new(FailingDecoder {
                inner: SampleDecoderFactory::default().open(p, g)?,
                fail_at: self.0,
                count: 0,
            }))
        }
    }
    impl Decoder for FailingDecoder {
        fn info(&self) -> &MediaInfo {
            self.inner.info()
        }
        fn seek(&mut self, t: RationalTime) -> Result<()> {
            self.count = 0;
            self.inner.seek(t)
        }
        fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
            if self.count == self.fail_at {
                return Err(MmError::Corrupt("hardware accelerator failed".into()));
            }
            self.count += 1;
            self.inner.next_frame()
        }
        fn decode_audio(&mut self, r: TimeRange) -> Result<AudioBlock> {
            self.inner.decode_audio(r)
        }
    }

    struct Ctx(mm_types::GpuAdapterId);
    impl GpuContextRef for Ctx {
        fn adapter(&self) -> &mm_types::GpuAdapterId {
            &self.0
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    fn ctx() -> Ctx {
        Ctx(mm_types::GpuAdapterId {
            vendor: 0,
            device: 0,
            name: "test".into(),
            luid: None,
        })
    }

    fn auto(gpu: Vec<Arc<dyn DecoderFactory>>) -> AutoDecoderFactory {
        AutoDecoderFactory::new(gpu, Some(Arc::new(SampleDecoderFactory::default())))
    }

    #[test]
    fn 開けない実装は飛ばして次を使う() {
        let a = auto(vec![Arc::new(CannotOpen), Arc::new(FailsAt(usize::MAX))]);
        let (_, chosen) = a
            .open_with_choice(Path::new("x.mp4"), Some(&ctx()))
            .unwrap();
        assert_eq!(chosen, "fails_at");
    }

    #[test]
    fn 開いた直後の数コマで落ちる実装は飛ばす() {
        for at in [0, PROBE_FRAMES - 1] {
            let a = auto(vec![Arc::new(FailsAt(at))]);
            let (_, chosen) = a
                .open_with_choice(Path::new("x.mp4"), Some(&ctx()))
                .unwrap();
            assert_eq!(
                chosen,
                mm_testkit::SAMPLE_ID,
                "{at} コマ目で落ちる実装は使わない"
            );
        }
    }

    #[test]
    fn 試したコマの後は先頭から出る() {
        let a = auto(vec![Arc::new(FailsAt(usize::MAX))]);
        let (mut dec, chosen) = a
            .open_with_choice(Path::new("x.mp4"), Some(&ctx()))
            .unwrap();
        assert_eq!(chosen, "fails_at");
        assert_eq!(dec.next_frame().unwrap().unwrap().index, 0);
    }

    #[test]
    fn gpu_の文脈が無ければ最初から逃げ道を使う() {
        let a = auto(vec![Arc::new(FailsAt(usize::MAX))]);
        let (_, chosen) = a.open_with_choice(Path::new("x.mp4"), None).unwrap();
        assert_eq!(chosen, mm_testkit::SAMPLE_ID);
    }

    #[test]
    fn 逃げ道が無ければ最後の失敗を返す() {
        let a = AutoDecoderFactory::new(vec![Arc::new(CannotOpen)], None);
        assert!(matches!(
            a.open_with_choice(Path::new("x.mp4"), Some(&ctx())),
            Err(MmError::Unsupported(_))
        ));
    }

    #[test]
    fn 壊れた素材は逃げ道の失敗で返る() {
        let a = auto(vec![Arc::new(CannotOpen)]);
        assert!(matches!(
            a.open_with_choice(Path::new("x.broken.mp4"), Some(&ctx())),
            Err(MmError::Corrupt(_))
        ));
    }

    #[test]
    fn 音は別の実装から来る() {
        let video = Arc::new(FailsAt(usize::MAX));
        let audio = Arc::new(SampleDecoderFactory::default());
        let mut dec = WithAudio::new(video, audio)
            .open(Path::new("x.mp4"), None)
            .unwrap();
        assert!(dec.info().audio.is_some());
        let range = TimeRange::new(RationalTime::ZERO, RationalTime::seconds(1)).unwrap();
        let block = dec.decode_audio(range).unwrap();
        assert!(block.frames() > 0);
    }
}
