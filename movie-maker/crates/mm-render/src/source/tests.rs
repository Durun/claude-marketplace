use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use mm_decode_api::{DecodedFrame, Decoder, DecoderFactory, GpuContextRef, MediaInfo};
use mm_testkit::decode::{SampleDecoderFactory, SampleMedia};
use mm_testkit::fixtures::vertical_project;
use mm_types::{AssetId, GpuAdapterId, Id, ParamValue, Params, RationalTime, Result, TimeRange};

use super::*;

/// 開いた数とシークの数を数えるモックのデコーダー。
#[derive(Default)]
struct Counts {
    opens: AtomicUsize,
    seeks: AtomicUsize,
}

struct Counting {
    inner: SampleDecoderFactory,
    counts: Arc<Counts>,
}

impl DecoderFactory for Counting {
    fn id(&self) -> &'static str {
        "counting"
    }

    fn open(&self, path: &Path, gpu: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>> {
        self.counts.opens.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(CountingDecoder {
            inner: self.inner.open(path, gpu)?,
            counts: self.counts.clone(),
        }))
    }
}

struct CountingDecoder {
    inner: Box<dyn Decoder>,
    counts: Arc<Counts>,
}

impl Decoder for CountingDecoder {
    fn info(&self) -> &MediaInfo {
        self.inner.info()
    }

    fn seek(&mut self, t: RationalTime) -> Result<()> {
        self.counts.seeks.fetch_add(1, Ordering::SeqCst);
        self.inner.seek(t)
    }

    fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        self.inner.next_frame()
    }

    fn decode_audio(&mut self, range: TimeRange) -> Result<AudioBlock> {
        self.inner.decode_audio(range)
    }
}

struct NoGpu(GpuAdapterId);

impl GpuContextRef for NoGpu {
    fn adapter(&self) -> &GpuAdapterId {
        &self.0
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct Rig {
    pool: SourcePool,
    factory: Counting,
    counts: Arc<Counts>,
    project: Project,
    params: Params,
    gpu: NoGpu,
    media: SampleMedia,
}

fn rig() -> Rig {
    let media = SampleMedia::default();
    let path = std::env::temp_dir().join("mm_render_source_pool.json");
    let (project, _, _) = vertical_project(&media, &path);
    let asset: AssetId = Id::new("clip");
    let mut params = Params::default();
    params.0.insert("asset".into(), ParamValue::AssetRef(asset));
    let counts = Arc::new(Counts::default());
    Rig {
        pool: SourcePool::default(),
        factory: Counting {
            inner: SampleDecoderFactory {
                media: media.clone(),
            },
            counts: counts.clone(),
        },
        counts,
        project,
        params,
        gpu: NoGpu(GpuAdapterId {
            vendor: 0,
            device: 0,
            name: "なし".into(),
            luid: None,
        }),
        media,
    }
}

impl Rig {
    /// 1 フレームの中で、素材の番号 `indices` を順に読む。
    fn frame(&mut self, indices: &[i64]) -> Vec<i64> {
        self.pool.begin_frame();
        indices
            .iter()
            .map(|&i| {
                let t = RationalTime::from_frames(i, self.media.fps).expect("表せる");
                let source = self
                    .pool
                    .footage_for(&self.factory, &self.gpu, &self.project, &self.params, t)
                    .expect("開ける")
                    .expect("素材がある");
                source.frame_at(t).expect("読める").index
            })
            .collect()
    }

    fn seeks(&self) -> usize {
        self.counts.seeks.load(Ordering::SeqCst)
    }

    fn opens(&self) -> usize {
        self.counts.opens.load(Ordering::SeqCst)
    }
}

#[test]
fn 続けて読むフレームはシークしない() {
    let mut r = rig();
    for i in 0..30 {
        assert_eq!(r.frame(&[i]), vec![i]);
    }
    assert_eq!((r.opens(), r.seeks()), (1, 1), "最初の 1 回だけシークする");
}

#[test]
fn 同じ素材を時刻をずらして重ねても_それぞれのデコーダーが読み進める() {
    let mut r = rig();
    for i in 0..30 {
        assert_eq!(r.frame(&[i, i + 40]), vec![i, i + 40]);
    }
    assert_eq!(r.opens(), 2, "重ねた数だけ開く");
    assert_eq!(r.seeks(), 2, "それぞれ最初の 1 回だけシークする");
}

#[test]
fn 入れ子を順に並べた素材の頭出しは開き直さずシークで済ませる() {
    let mut r = rig();
    for _ in 0..3 {
        for i in 0..10 {
            r.frame(&[i]);
        }
    }
    assert_eq!(r.opens(), 1);
    assert_eq!(r.seeks(), 3, "先頭に戻るたびに 1 回");
}

#[test]
fn 重ねる数が上限を超えたら最も前に使ったデコーダーをシークし直す() {
    let mut r = rig();
    let layers = MAX_DECODERS_PER_ASSET as i64 + 1;
    let at = |i: i64| (0..layers).map(|k| i + k * 20).collect::<Vec<_>>();
    for i in 0..5 {
        assert_eq!(r.frame(&at(i)), at(i));
    }
    assert_eq!(r.opens(), MAX_DECODERS_PER_ASSET);
}

#[test]
fn 長く使わないデコーダーは閉じ_次に使うときに開き直す() {
    let mut r = rig();
    r.frame(&[0]);
    for _ in 0..IDLE_FRAMES * 2 {
        r.pool.begin_frame();
    }
    assert!(
        r.pool.sources.is_empty(),
        "使わない素材のデコーダーを閉じた"
    );
    assert_eq!(r.frame(&[1]), vec![1]);
    assert_eq!(r.opens(), 2, "閉じた素材は開き直す");
}

#[test]
fn 使い続けるデコーダーは閉じない() {
    let mut r = rig();
    for i in 0..(IDLE_FRAMES as i64 * 2) {
        r.frame(&[i % 30]);
    }
    assert_eq!(r.opens(), 1);
}
