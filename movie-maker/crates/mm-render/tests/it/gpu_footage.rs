//! GPU のフレームを返すデコーダーを footage が受ける。素材のままの大きさのテクスチャを、
//! レンダラーが画布に収めて（縦横比を保ち、縮小は面積平均で）置く。
//! GPU が無い環境ではスキップし、`MM_REQUIRE_GPU` があれば失敗にする。

use std::path::Path;
use std::sync::Arc;

use mm_audio_api::AudioBlock;
use mm_decode_api::{DecodedFrame, Decoder, DecoderFactory, GpuContextRef, MediaInfo};
use mm_doc::{Asset, Composition, Layer, Node, Project};
use mm_gpu::{GpuFrameTexture, WgpuGpu};
use mm_node_api::{FrameRef, GpuHelper, Quality};
use mm_nodes_basic::all_kinds;
use mm_render::{GpuContext, GpuRequest, WgpuRenderer};
use mm_render_api::{RenderRequest, Renderer};
use mm_testkit::audio::SampleMixerFactory;
use mm_testkit::decode::{SampleDecoderFactory, SampleMedia};
use mm_testkit::fixtures::{sample_services, SAMPLE_ASSET_PATH};
use mm_testkit::node::MapRegistry;
use mm_types::{
    ColorSpace, Id, ParamValue, Rational, RationalTime, Result, Rgba, RgbaImage, Size, TimeRange,
};

/// f16 の丸めと面積平均の誤差の許容。
const TOLERANCE: f32 = 3e-3;

/// 160x160 の画布に収めた 320x180 の素材の矩形の上端と下端（y）。
const BOX_TOP: u32 = 35;
const BOX_BOTTOM: u32 = 125;
/// 縮小の補間の違いが出る、矩形の縁からの幅（画素）。
const EDGE_BAND: u32 = 2;

fn gpu() -> Option<Arc<GpuContext>> {
    match GpuContext::shared(&GpuRequest::default()) {
        Ok(ctx) => Some(ctx),
        Err(e) if std::env::var_os("MM_REQUIRE_GPU").is_some() => {
            panic!("MM_REQUIRE_GPU が有るのに GPU を作れない: {e}")
        }
        Err(e) => {
            eprintln!("GPU が無いのでスキップする: {e}");
            None
        }
    }
}

/// モックのデコーダーの CPU のフレームを、テクスチャにして GPU のフレームとして返すデコーダー。
struct GpuSampleDecoder {
    inner: Box<dyn Decoder>,
    gpu: WgpuGpu,
    next_id: u64,
}

impl Decoder for GpuSampleDecoder {
    fn info(&self) -> &MediaInfo {
        self.inner.info()
    }

    fn seek(&mut self, t: RationalTime) -> Result<()> {
        self.inner.seek(t)
    }

    fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        let Some(frame) = self.inner.next_frame()? else {
            return Ok(None);
        };
        let image = frame.image.expect("モックは CPU のフレーム");
        let FrameRef::Gpu(tex) = self.gpu.upload(&image)? else {
            unreachable!("upload は Gpu を返す");
        };
        let texture = self.gpu.texture(tex.id).expect("作った").clone();
        self.gpu.release_id(tex.id);
        let id = self.next_id;
        self.next_id += 1;
        let handle = GpuFrameTexture {
            texture,
            wait: None,
        }
        .into_handle(id);
        Ok(Some(DecodedFrame {
            index: frame.index,
            pts: frame.pts,
            image: None,
            gpu: Some(handle),
        }))
    }

    fn decode_audio(&mut self, range: TimeRange) -> Result<AudioBlock> {
        self.inner.decode_audio(range)
    }
}

struct GpuSampleFactory;

impl DecoderFactory for GpuSampleFactory {
    fn id(&self) -> &'static str {
        "gpu_sample"
    }

    fn open(&self, path: &Path, gpu: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>> {
        let ctx = gpu
            .and_then(|g| g.as_any().downcast_ref::<GpuContext>())
            .expect("レンダラーのコンテキスト");
        Ok(Box::new(GpuSampleDecoder {
            inner: SampleDecoderFactory::default().open(path, None)?,
            gpu: WgpuGpu::new(GpuContext::shared(ctx.request()).expect("共有")),
            next_id: 0,
        }))
    }
}

fn project(size: Size) -> Project {
    let media = SampleMedia::default();
    let mut p = Project::new();
    p.assets.push(Asset {
        id: Id::new("clip"),
        path: SAMPLE_ASSET_PATH.into(),
        content_hash: "sample0000000000".into(),
        color_space: ColorSpace::bt709(),
        duration: media.duration,
        fps: Some(media.fps),
        timebase_offset: RationalTime::ZERO,
    });
    let footage =
        Node::new("footage", 1).with_param("asset", ParamValue::AssetRef(Id::new("clip")));
    p.compositions.push(
        Composition::new(
            Id::new("main"),
            size.width,
            size.height,
            Rational::integer(30),
            RationalTime::seconds(2),
        )
        .with_layers(vec![Layer::new(
            "f",
            footage,
            RationalTime::ZERO,
            RationalTime::ZERO,
            RationalTime::seconds(2),
        )]),
    );
    p
}

fn render(ctx: &Arc<GpuContext>, decoders: Arc<dyn DecoderFactory>, size: Size) -> Arc<RgbaImage> {
    let mut nodes = MapRegistry::default();
    for kind in all_kinds() {
        nodes.insert(kind);
    }
    let p = project(size);
    let mut r = WgpuRenderer::new(
        ctx.clone(),
        decoders,
        Arc::new(SampleMixerFactory),
        sample_services(),
    );
    let frame = r
        .render_frame(&RenderRequest {
            project: &p,
            composition: &Id::new("main"),
            time: RationalTime::from_frames(1, Rational::integer(30)).expect("表せる"),
            size,
            quality: Quality::Full,
            nodes: &nodes,
        })
        .expect("描ける");
    let img = r.gpu().download(&frame).expect("リードバックできる");
    assert!(
        r.take_errors().is_empty(),
        "footage が GPU のフレームを受ける"
    );
    // 持っているのは出力と、キャッシュが覚えた画像だけ。借用したフレームは残らない。
    let kept = 1 + r.frame_cache_stats().entries;
    assert_eq!(r.texture_count(), kept, "借用したフレームは提出の後に返す");
    img
}

#[test]
fn 同じ大きさなら_cpu_の経路と同じ画像() {
    let Some(ctx) = gpu() else { return };
    let size = SampleMedia::default().size;
    let want = render(&ctx, Arc::new(SampleDecoderFactory::default()), size);
    let got = render(&ctx, Arc::new(GpuSampleFactory), size);
    let diff = got.max_diff(&want).expect("同じ大きさ");
    assert!(diff <= TOLERANCE, "CPU の経路との差 {diff}");
}

#[test]
fn 縦横比の違う画布には収めて置き_縮小は面積平均で色を保つ() {
    let Some(ctx) = gpu() else { return };
    // 320x180 の素材を 160x160 に収める: 幅 160、高さ 90、上下に 35 の余白。
    let size = Size::new(160, 160);
    let got = render(&ctx, Arc::new(GpuSampleFactory), size);
    let p = |x, y| got.pixel(x, y);
    assert_eq!(p(80, 10).a, 0.0, "上の余白は透明");
    assert_eq!(p(80, 150).a, 0.0, "下の余白は透明");
    let cpu = render(&ctx, Arc::new(SampleDecoderFactory::default()), size);
    // 素材は一色。CPU は双線形、GPU は面積平均なので差が出るのは矩形の縁（y が 35 と 125）だけで、
    // それを除いた内側と余白は色まで CPU の経路と一致する。
    let near_edge =
        |y: u32| y.abs_diff(BOX_TOP) <= EDGE_BAND || y.abs_diff(BOX_BOTTOM) <= EDGE_BAND;
    for y in (0..size.height).filter(|&y| !near_edge(y)) {
        for x in 0..size.width {
            let (a, b) = (got.pixel(x, y), cpu.pixel(x, y));
            assert!(
                a.max_diff(b) <= TOLERANCE,
                "({x}, {y}) が CPU の経路と違う: {a:?} と {b:?}"
            );
        }
    }
    let inside = p(80, 80);
    assert!(inside.a > 0.99, "素材の中は塗られている: {inside:?}");
    assert!(cpu.pixel(80, 80).a > 0.99, "比べる側も塗られている");
    assert_eq!(cpu.pixel(80, 10).a, 0.0);
    assert!(
        got.pixel(80, 45).max_diff(Rgba::TRANSPARENT) > 0.0,
        "収めた素材の上端は塗られている"
    );
}
