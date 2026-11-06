//! デコードの選択肢の切り替え。GPU でデコードする実装を素材ごとに試し、だめなら次へフォールバックする。
//!
//! - [`AutoDecoderFactory`]: `d3d12va` → `cuvid` → `ffmpeg` の順に、素材ごとに開く。開いた直後の数フレームを
//!   デコードして通った実装を使う（NVENC で作った 4K の HEVC は d3d12va が 1 フレーム目でクラッシュする）。
//!   GPU のコンテキストが無ければ最初から `ffmpeg`。
//! - [`WithAudio`]: 映像を GPU の実装、音と素材の情報（音のストリーム、先頭のずれ）を別のデコーダーが受け持つ。
//!   GPU の実装は音を持たないため。

use std::path::Path;
use std::sync::Arc;

use mm_audio_api::AudioBlock;
use mm_decode_api::{DecodedFrame, Decoder, DecoderFactory, GpuContextRef, MediaInfo};
use mm_types::{MmError, RationalTime, Result, TimeRange};

// 開いた直後にデコードして確かめるフレーム数。0 フレーム目でクラッシュする素材だけでなく、数フレーム後にクラッシュするものも拾う。
// d3d12va が別プロセスで試すフレーム数と同じにそろえる。
#[cfg(feature = "d3d12va")]
use mm_decode_d3d12va::PROBE_FRAMES;
#[cfg(not(feature = "d3d12va"))]
const PROBE_FRAMES: usize = 4;

/// `auto` が試す GPU の実装 1 つ。
pub struct GpuCandidate {
    factory: Arc<dyn DecoderFactory>,
    probed_on_open: bool,
}

impl GpuCandidate {
    /// `open` ではデコードしない実装。`auto` が開いた直後の数フレームをデコードして確かめる。
    pub fn new(factory: Arc<dyn DecoderFactory>) -> Self {
        Self {
            factory,
            probed_on_open: false,
        }
    }

    /// `open` の中で、クラッシュする素材を別プロセスで試し済みの実装。`auto` は試しデコードを重ねない。
    pub fn probed_on_open(factory: Arc<dyn DecoderFactory>) -> Self {
        Self {
            factory,
            probed_on_open: true,
        }
    }
}

/// 素材ごとに GPU の実装を試して、通ったものを使うファクトリー。
pub struct AutoDecoderFactory {
    /// 試す順に並べた GPU の実装。
    gpu: Vec<GpuCandidate>,
    /// 最後のフォールバック（CPU）。
    fallback: Option<Arc<dyn DecoderFactory>>,
}

impl AutoDecoderFactory {
    /// `gpu` を前から順に試し、全部だめなら `fallback`。
    pub fn new(gpu: Vec<GpuCandidate>, fallback: Option<Arc<dyn DecoderFactory>>) -> Self {
        Self { gpu, fallback }
    }

    /// 素材を開き、どの実装が通ったか（`id`）も返す。
    pub fn open_with_choice(
        &self,
        path: &Path,
        gpu: Option<&dyn GpuContextRef>,
    ) -> Result<(Box<dyn Decoder>, &'static str)> {
        let mut last_error = None;
        for candidate in self.gpu.iter().filter(|_| gpu.is_some()) {
            let factory = &candidate.factory;
            match open_and_probe(candidate, path, gpu) {
                Ok(dec) => return Ok((dec, factory.id())),
                Err(e) => {
                    tracing::warn!(backend = factory.id(), path = %path.display(), error = %e, "GPU のデコードを使えない。次の実装を試す");
                    last_error = Some(e);
                }
            }
        }
        match &self.fallback {
            Some(f) => Ok((f.open(path, gpu)?, f.id())),
            None => Err(last_error.unwrap_or_else(|| {
                MmError::Unsupported(
                    "使えるデコーダーが無い（GPU のコンテキストか ffmpeg が要る）".into(),
                )
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

/// 開いて先頭の数フレームをデコードし、通れば最初の位置に戻して返す。`open` の中で試し済みの実装は開くだけ。
fn open_and_probe(
    candidate: &GpuCandidate,
    path: &Path,
    gpu: Option<&dyn GpuContextRef>,
) -> Result<Box<dyn Decoder>> {
    let mut dec = candidate.factory.open(path, gpu)?;
    if candidate.probed_on_open {
        return Ok(dec);
    }
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
    fn backend_id(&self) -> Option<&'static str> {
        self.video.backend_id()
    }

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

/// 映像の実装に、音を別の実装で付けるファクトリー。
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

    /// 開けるが、`fail_at` 番目のフレームでクラッシュする実装。
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
        let gpu = gpu.into_iter().map(GpuCandidate::new).collect();
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
    fn 開いた直後の数フレームでクラッシュする実装はスキップする() {
        for at in [0, PROBE_FRAMES - 1] {
            let a = auto(vec![Arc::new(FailsAt(at))]);
            let (_, chosen) = a
                .open_with_choice(Path::new("x.mp4"), Some(&ctx()))
                .unwrap();
            assert_eq!(
                chosen,
                mm_testkit::SAMPLE_ID,
                "{at} フレーム目でクラッシュする実装は使わない"
            );
        }
    }

    #[test]
    fn 試したフレームの後は先頭から出る() {
        let a = auto(vec![Arc::new(FailsAt(usize::MAX))]);
        let (mut dec, chosen) = a
            .open_with_choice(Path::new("x.mp4"), Some(&ctx()))
            .unwrap();
        assert_eq!(chosen, "fails_at");
        assert_eq!(dec.next_frame().unwrap().unwrap().index, 0);
    }

    #[test]
    fn gpu_のコンテキストが無ければ最初からフォールバックを使う() {
        let a = auto(vec![Arc::new(FailsAt(usize::MAX))]);
        let (_, chosen) = a.open_with_choice(Path::new("x.mp4"), None).unwrap();
        assert_eq!(chosen, mm_testkit::SAMPLE_ID);
    }

    #[test]
    fn フォールバックが無ければ最後の失敗を返す() {
        let a = AutoDecoderFactory::new(vec![GpuCandidate::new(Arc::new(CannotOpen))], None);
        assert!(matches!(
            a.open_with_choice(Path::new("x.mp4"), Some(&ctx())),
            Err(MmError::Unsupported(_))
        ));
    }

    #[test]
    fn 壊れた素材はフォールバックの失敗で返る() {
        let a = auto(vec![Arc::new(CannotOpen)]);
        assert!(matches!(
            a.open_with_choice(Path::new("x.broken.mp4"), Some(&ctx())),
            Err(MmError::Corrupt(_))
        ));
    }

    /// `open` で何フレームデコードされたかを数える実装。
    struct Counting(Arc<std::sync::atomic::AtomicUsize>);
    struct CountingDecoder {
        inner: Box<dyn Decoder>,
        decoded: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl DecoderFactory for Counting {
        fn id(&self) -> &'static str {
            "counting"
        }
        fn open(&self, p: &Path, g: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>> {
            Ok(Box::new(CountingDecoder {
                inner: SampleDecoderFactory::default().open(p, g)?,
                decoded: self.0.clone(),
            }))
        }
    }
    impl Decoder for CountingDecoder {
        fn info(&self) -> &MediaInfo {
            self.inner.info()
        }
        fn seek(&mut self, t: RationalTime) -> Result<()> {
            self.inner.seek(t)
        }
        fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
            self.decoded
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.next_frame()
        }
        fn decode_audio(&mut self, r: TimeRange) -> Result<AudioBlock> {
            self.inner.decode_audio(r)
        }
    }

    fn open_with(candidate: GpuCandidate) {
        let a = AutoDecoderFactory::new(vec![candidate], None);
        let (_, chosen) = a
            .open_with_choice(Path::new("x.mp4"), Some(&ctx()))
            .unwrap();
        assert_eq!(chosen, "counting");
    }

    #[test]
    fn open_で試し済みの実装には試しデコードを重ねない() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let count = Arc::new(AtomicUsize::new(0));
        open_with(GpuCandidate::probed_on_open(Arc::new(Counting(
            count.clone(),
        ))));
        assert_eq!(count.load(Ordering::SeqCst), 0);
        open_with(GpuCandidate::new(Arc::new(Counting(count.clone()))));
        assert_eq!(count.load(Ordering::SeqCst), PROBE_FRAMES);
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
