//! 同じフレームの中で、同じ素材の N と N+1 を同じデコーダーに読ませても（読み進めの選び方）、N の GPU のフレームが
//! 提出を待つ間に上書きされない。デコーダーはプールのテクスチャを貸し、借用が返ったら使い回す（`mm-decode-hw` の
//! `FramePool` と同じ形。cuvid と d3d12va はこの形）。
//! GPU が無い環境ではスキップし、`MM_REQUIRE_GPU` があれば失敗にする。

use std::path::Path;
use std::sync::{Arc, Mutex};

use half::f16;
use mm_audio_api::AudioBlock;
use mm_decode_api::{DecodedFrame, Decoder, DecoderFactory, GpuContextRef, MediaInfo};
use mm_doc::{Asset, Composition, Layer, Node, Param, Project};
use mm_gpu::GpuFrameTexture;
use mm_node_api::Quality;
use mm_nodes_basic::all_kinds;
use mm_render::{FrameCacheConfig, GpuContext, GpuRequest, WgpuRenderer, FRAME_FORMAT};
use mm_render_api::{RenderRequest, Renderer};
use mm_testkit::audio::SampleMixerFactory;
use mm_testkit::decode::{SampleDecoderFactory, SampleMedia};
use mm_testkit::fixtures::{sample_services, SAMPLE_ASSET_PATH};
use mm_testkit::node::MapRegistry;
use mm_types::{
    ColorSpace, Id, ParamValue, Rational, RationalTime, Result, RgbaImage, Size, TimeRange,
};

const SIZE: Size = Size::new(64, 36);
/// 2 つのレイヤーが読む素材のフレーム（隣り合う）。
const FIRST: i64 = 10;
/// f16 の丸めと縮小の許容。
const TOLERANCE: f32 = 3e-3;
/// 1 画素のバイト数（RGBA の f16）。
const BYTES_PER_PIXEL: u32 = 8;

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

type Free = Arc<Mutex<Vec<wgpu::Texture>>>;

/// 借用が返ったらテクスチャをプールに戻す。
struct Return {
    free: Free,
    texture: wgpu::Texture,
}

impl Drop for Return {
    fn drop(&mut self) {
        if let Ok(mut free) = self.free.lock() {
            free.push(self.texture.clone());
        }
    }
}

/// モックのデコーダーの CPU のフレームを、プールのテクスチャに書いて貸すデコーダー。
struct PoolDecoder {
    inner: Box<dyn Decoder>,
    ctx: Arc<GpuContext>,
    free: Free,
    next_id: u64,
}

impl PoolDecoder {
    fn take(&self, size: Size) -> wgpu::Texture {
        let reused = self.free.lock().ok().and_then(|mut f| f.pop());
        reused.unwrap_or_else(|| {
            self.ctx.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("pool frame"),
                size: wgpu::Extent3d {
                    width: size.width,
                    height: size.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FRAME_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        })
    }

    fn write(&self, texture: &wgpu::Texture, image: &RgbaImage) {
        let halves: Vec<u8> = image
            .data()
            .iter()
            .flat_map(|v| f16::from_f32(*v).to_le_bytes())
            .collect();
        let size = image.size();
        self.ctx.queue.write_texture(
            texture.as_image_copy(),
            &halves,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width * BYTES_PER_PIXEL),
                rows_per_image: Some(size.height),
            },
            texture.size(),
        );
        // 書き込みはこの提出で GPU に届く。デコーダーが自前で提出するのと同じ順序にする。
        self.ctx.queue.submit([]);
    }
}

impl Decoder for PoolDecoder {
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
        let texture = self.take(image.size());
        self.write(&texture, &image);
        let guard = Return {
            free: self.free.clone(),
            texture: texture.clone(),
        };
        let handle = GpuFrameTexture {
            texture,
            wait: Some(Box::new(move |_queue| {
                let _keep = &guard;
            })),
        }
        .into_handle(self.next_id);
        self.next_id += 1;
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

struct PoolFactory;

impl DecoderFactory for PoolFactory {
    fn id(&self) -> &'static str {
        "pool_sample"
    }

    fn open(&self, path: &Path, gpu: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>> {
        let ctx = gpu
            .and_then(|g| g.as_any().downcast_ref::<GpuContext>())
            .expect("レンダラーのコンテキスト");
        Ok(Box::new(PoolDecoder {
            inner: SampleDecoderFactory::default().open(path, None)?,
            ctx: GpuContext::shared(ctx.request()).expect("共有"),
            free: Arc::default(),
            next_id: 0,
        }))
    }
}

/// 素材のフレーム `FIRST` を画布の全体に、`FIRST + 1` を半分の大きさで中央に重ねる。
fn project() -> Project {
    let media = SampleMedia::default();
    let fps = media.fps;
    let mut p = Project::new();
    p.assets.push(Asset {
        id: Id::new("clip"),
        path: SAMPLE_ASSET_PATH.into(),
        content_hash: "sample0000000000".into(),
        color_space: ColorSpace::bt709(),
        duration: media.duration,
        fps: Some(fps),
        timebase_offset: RationalTime::ZERO,
    });
    let footage =
        Node::new("footage", 1).with_param("asset", ParamValue::AssetRef(Id::new("clip")));
    let layer = |id: &str, frame: i64| {
        let at = RationalTime::from_frames(frame, fps).expect("表せる");
        let out = at.checked_add(RationalTime::seconds(1)).expect("表せる");
        Layer::new(id, footage.clone(), RationalTime::ZERO, at, out)
    };
    let mut top = layer("next", FIRST + 1);
    top.transform.scale = Param::constant(ParamValue::Number(0.5));
    p.compositions.push(
        Composition::new(
            Id::new("main"),
            SIZE.width,
            SIZE.height,
            Rational::integer(30),
            RationalTime::seconds(1),
        )
        .with_layers(vec![layer("first", FIRST), top]),
    );
    p
}

fn render(r: &mut WgpuRenderer, p: &Project) -> Arc<RgbaImage> {
    let mut nodes = MapRegistry::default();
    for kind in all_kinds() {
        nodes.insert(kind);
    }
    let frame = r
        .render_frame(&RenderRequest {
            project: p,
            composition: &Id::new("main"),
            time: RationalTime::ZERO,
            size: SIZE,
            quality: Quality::Full,
            nodes: &nodes,
        })
        .expect("描ける");
    assert!(r.take_errors().is_empty());
    r.gpu().download(&frame).expect("リードバックできる")
}

fn renderer(ctx: &Arc<GpuContext>, decoders: Arc<dyn DecoderFactory>) -> WgpuRenderer {
    WgpuRenderer::new(
        ctx.clone(),
        decoders,
        Arc::new(SampleMixerFactory),
        sample_services(),
    )
}

#[test]
fn 同じデコーダーで隣のフレームを読んでも前のフレームが化けない() {
    let Some(ctx) = gpu() else { return };
    let p = project();
    // CPU のフレームを返すデコーダーで描いた画像を正とする。
    let want = render(
        &mut renderer(&ctx, Arc::new(SampleDecoderFactory::default())),
        &p,
    );
    let no_cache = FrameCacheConfig {
        memory_bytes: 0,
        store: None,
    };
    for (name, mut r) in [
        ("キャッシュあり", renderer(&ctx, Arc::new(PoolFactory))),
        (
            "キャッシュなし",
            renderer(&ctx, Arc::new(PoolFactory)).with_frame_cache(no_cache),
        ),
    ] {
        for frame in 0..3 {
            let got = render(&mut r, &p);
            let diff = got.max_diff(&want).expect("同じ大きさ");
            assert!(diff <= TOLERANCE, "{name} の {frame} 枚目: 差 {diff}");
        }
    }
}
