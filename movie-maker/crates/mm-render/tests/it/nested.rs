//! 入れ子の音と、入れ子の中のベクターの倍率、連続した音のミキサーの持ち越し。
//! GPU が無い環境では飛ばし、`MM_REQUIRE_GPU` があれば失敗にする。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use mm_audio_api::{AudioBlock, Mixer, MixerConfig, MixerFactory};
use mm_doc::{Asset, Composition, Layer, Node, Param, Project};
use mm_node_api::{FrameCx, FrameRef, NodeKind, Quality, Traits};
use mm_nodes_basic::all_kinds;
use mm_render::{GpuContext, GpuRequest, WgpuRenderer};
use mm_render_api::{AudioRequest, RenderRequest, Renderer};
use mm_testkit::audio::SampleMixerFactory;
use mm_testkit::decode::{SampleDecoderFactory, SampleMedia};
use mm_testkit::fixtures::{sample_services, SAMPLE_ASSET_PATH};
use mm_testkit::node::MapRegistry;
use mm_types::Rgba;
use mm_types::{
    ColorSpace, Id, ParamSchema, ParamValue, Params, Rational, RationalTime, Result, Size,
    TimeRange,
};

const RATE: u32 = mm_audio_api::DEFAULT_SAMPLE_RATE;
const CHANNELS: u16 = mm_audio_api::DEFAULT_CHANNELS;
const SMALL: Size = Size::new(64, 36);

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

/// 描かれたときの倍率を記録するだけのベクターの種類のノード。
struct Probe(Arc<Mutex<Vec<f32>>>);

impl NodeKind for Probe {
    fn id(&self) -> &'static str {
        "probe"
    }

    fn version(&self) -> u32 {
        1
    }

    fn params(&self) -> ParamSchema {
        ParamSchema::new(vec![])
    }

    fn traits(&self) -> Traits {
        Traits {
            vector: true,
            ..Traits::PIXEL
        }
    }

    fn migrate(&self, _old: serde_json::Value, _from: u32) -> Result<Params> {
        Ok(Params::default())
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        self.0.lock().expect("ロック").push(cx.scale);
        Ok(FrameRef::Transparent { size: cx.size })
    }
}

fn registry(scales: &Arc<Mutex<Vec<f32>>>) -> MapRegistry {
    let mut r = MapRegistry::default();
    for kind in all_kinds() {
        r.insert(kind);
    }
    r.insert(Arc::new(Probe(scales.clone())));
    r
}

/// 作った回数を数えるミキサーの工場。
#[derive(Default)]
struct CountingFactory {
    created: AtomicUsize,
}

impl MixerFactory for CountingFactory {
    fn id(&self) -> &'static str {
        "counting"
    }

    fn create(&self, rate: u32, channels: u16, config: &MixerConfig) -> Result<Box<dyn Mixer>> {
        self.created.fetch_add(1, Ordering::SeqCst);
        SampleMixerFactory.create(rate, channels, config)
    }
}

fn renderer(ctx: &Arc<GpuContext>, mixers: Arc<dyn MixerFactory>) -> WgpuRenderer {
    WgpuRenderer::new(
        ctx.clone(),
        Arc::new(SampleDecoderFactory::default()),
        mixers,
        sample_services(),
    )
}

fn layer(id: &str, source: Node, start: RationalTime, length: RationalTime) -> Layer {
    Layer::new(id, source, start, RationalTime::ZERO, length)
}

fn comp(id: &str, size: Size, layers: Vec<Layer>) -> Composition {
    Composition::new(
        Id::new(id),
        size.width,
        size.height,
        Rational::integer(30),
        RationalTime::seconds(4),
    )
    .with_layers(layers)
}

fn project(comps: Vec<Composition>) -> Project {
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
    p.compositions = comps;
    p
}

fn nested_node(name: &str) -> Node {
    Node::new("composition", 1).with_param("composition", ParamValue::Text(name.into()))
}

fn footage_node() -> Node {
    Node::new("footage", 1).with_param("asset", ParamValue::AssetRef(Id::new("clip")))
}

fn span(from_ms: i128, to_ms: i128) -> TimeRange {
    TimeRange::new(
        RationalTime::new(from_ms, 1000).expect("表せる"),
        RationalTime::new(to_ms, 1000).expect("表せる"),
    )
    .expect("範囲")
}

fn audio(
    r: &mut WgpuRenderer,
    p: &Project,
    nodes: &MapRegistry,
    name: &str,
    range: TimeRange,
) -> AudioBlock {
    r.render_audio(&AudioRequest {
        project: p,
        composition: &Id::new(name),
        range,
        sample_rate: RATE,
        channels: CHANNELS,
        nodes,
    })
    .expect("音を混ぜられる")
}

#[test]
fn 入れ子の中の音が親の時間に置かれて出る() {
    let Some(ctx) = gpu() else { return };
    let one_sec = RationalTime::seconds(1);
    let inner = comp(
        "inner",
        SMALL,
        vec![layer("f", footage_node(), RationalTime::ZERO, one_sec)],
    );
    // 入れ子のレイヤーは親の 0.5 秒から始まる。
    let half = RationalTime::new(1, 2).expect("表せる");
    let outer = comp(
        "outer",
        SMALL,
        vec![layer("n", nested_node("inner"), half, one_sec)],
    );
    let p = project(vec![inner, outer]);
    let nodes = registry(&Arc::default());
    let mut r = renderer(&ctx, Arc::new(SampleMixerFactory));
    let want = audio(&mut r, &p, &nodes, "inner", span(0, 500));
    assert!(!want.is_silent(), "中の音は鳴っている");
    let got = audio(&mut r, &p, &nodes, "outer", span(500, 1000));
    assert_eq!(got.max_diff(&want), Some(0.0), "入れ子の音が中と同じ");
    let before = audio(&mut r, &p, &nodes, "outer", span(1000, 1500));
    assert!(before.frames() > 0, "区間の長さはそのまま");
    let head = audio(&mut r, &p, &nodes, "outer", span(0, 500));
    assert!(head.is_silent(), "入れ子が始まる前は無音");
}

#[test]
fn sequence_のカットの音が素材のイン点から並んで出る() {
    let Some(ctx) = gpu() else { return };
    let ms = |v: i128| RationalTime::new(v, 1000).expect("表せる");
    // 素材の [1.0, 1.5) と [0.2, 0.7) を、この順に並べる。
    let first = mm_doc::Cut::new("k1", footage_node(), ms(1000), ms(1500));
    let second = mm_doc::Cut::new("k2", footage_node(), ms(200), ms(700));
    let seq_node = Node::new("sequence", 1).with_param("sequence", ParamValue::Text("s1".into()));
    let direct = |id: &str, from: i128, to: i128| {
        Layer::new(id, footage_node(), RationalTime::ZERO, ms(from), ms(to))
    };
    let mut p = project(vec![
        comp(
            "main",
            SMALL,
            vec![layer("q", seq_node, RationalTime::ZERO, ms(1000))],
        ),
        comp("a", SMALL, vec![direct("fa", 1000, 1500)]),
        comp("b", SMALL, vec![direct("fb", 200, 700)]),
    ]);
    p.sequences.push(mm_doc::Sequence {
        id: Id::new("s1"),
        cuts: vec![first, second],
        origin: None,
    });
    let nodes = registry(&Arc::default());
    let mut r = renderer(&ctx, Arc::new(SampleMixerFactory));
    let want_a = audio(&mut r, &p, &nodes, "a", span(0, 500));
    let want_b = audio(&mut r, &p, &nodes, "b", span(0, 500));
    assert!(!want_a.is_silent() && want_a.max_diff(&want_b) != Some(0.0));
    let got_a = audio(&mut r, &p, &nodes, "main", span(0, 500));
    let got_b = audio(&mut r, &p, &nodes, "main", span(500, 1000));
    assert_eq!(got_a.max_diff(&want_a), Some(0.0), "1 つ目のカットの音");
    assert_eq!(got_b.max_diff(&want_b), Some(0.0), "2 つ目のカットの音");
}

#[test]
fn 続きの区間はミキサーを持ち越しシークで作り直す() {
    let Some(ctx) = gpu() else { return };
    let p = project(vec![comp(
        "main",
        SMALL,
        vec![layer(
            "f",
            footage_node(),
            RationalTime::ZERO,
            RationalTime::seconds(3),
        )],
    )]);
    let nodes = registry(&Arc::default());
    let factory = Arc::new(CountingFactory::default());
    let mut r = renderer(&ctx, factory.clone());
    for (from, to) in [(0, 100), (100, 200), (200, 300)] {
        audio(&mut r, &p, &nodes, "main", span(from, to));
    }
    assert_eq!(
        factory.created.load(Ordering::SeqCst),
        1,
        "続きは同じミキサー"
    );
    audio(&mut r, &p, &nodes, "main", span(1000, 1100));
    assert_eq!(factory.created.load(Ordering::SeqCst), 2, "飛んだらシーク");
    audio(&mut r, &p, &nodes, "main", span(1100, 1200));
    assert_eq!(factory.created.load(Ordering::SeqCst), 2);
}

#[test]
fn 親が拡大した入れ子の中のベクターはその分細かく描く() {
    let Some(ctx) = gpu() else { return };
    let scales = Arc::new(Mutex::new(Vec::new()));
    let nodes = registry(&scales);
    let two = RationalTime::seconds(2);
    let inner = comp(
        "inner",
        SMALL,
        vec![layer("p", Node::new("probe", 1), RationalTime::ZERO, two)],
    );
    let mut big = layer("n", nested_node("inner"), RationalTime::ZERO, two);
    big.transform.scale = Param::constant(ParamValue::Number(3.0));
    let p = project(vec![inner, comp("outer", SMALL, vec![big])]);
    let mut r = renderer(&ctx, Arc::new(SampleMixerFactory));
    let id = Id::new("outer");
    r.render_frame(&RenderRequest {
        project: &p,
        composition: &id,
        time: RationalTime::ZERO,
        size: SMALL,
        quality: Quality::Full,
        nodes: &nodes,
    })
    .expect("描ける");
    let seen = scales.lock().expect("ロック").clone();
    assert_eq!(seen.len(), 1);
    assert!(
        (seen[0] - 3.0).abs() < 0.05,
        "親の 3 倍が入る（{}）",
        seen[0]
    );
}

#[test]
fn 親が3倍の入れ子の文字を_png_に書く() {
    let Some(ctx) = gpu() else { return };
    let size = Size::new(640, 360);
    let two = RationalTime::seconds(2);
    let text = Node::new("text", 1)
        .with_param("text", ParamValue::Text("Sharp".into()))
        .with_param("size", ParamValue::Number(24.0))
        .with_param("position", ParamValue::Vec2([6.0, 10.0]));
    let teal = ParamValue::Color(Rgba::new(0.0, 0.3, 0.3, 1.0));
    let inner = comp(
        "badge",
        Size::new(160, 60),
        vec![
            layer(
                "bg",
                Node::new("solid", 1).with_param("color", teal),
                RationalTime::ZERO,
                two,
            ),
            layer("t", text, RationalTime::ZERO, two),
        ],
    );
    let mut big = layer("n", nested_node("badge"), RationalTime::ZERO, two);
    big.transform.scale = Param::constant(ParamValue::Number(3.0));
    big.transform.anchor = Some(Param::constant(ParamValue::Vec2([0.0, 0.0])));
    big.transform.position = Param::constant(ParamValue::Vec2([20.0, 20.0]));
    let p = project(vec![inner, comp("main", size, vec![big])]);
    let nodes = registry(&Arc::default());
    let engine = mm_text_glyphon::TextEngine::new();
    let mut services = sample_services();
    services.text_shaper = engine.shaper();
    services.text_rasterizer = engine.rasterizer();
    let mut r = WgpuRenderer::new(
        ctx.clone(),
        Arc::new(SampleDecoderFactory::default()),
        Arc::new(SampleMixerFactory),
        services,
    );
    let id = Id::new("main");
    let frame = r
        .render_frame(&RenderRequest {
            project: &p,
            composition: &id,
            time: RationalTime::ZERO,
            size,
            quality: Quality::Full,
            nodes: &nodes,
        })
        .expect("描ける");
    let img = r.gpu().download(&frame).expect("読み戻せる");
    let rgb: Vec<u8> = img
        .data()
        .chunks(4)
        .flat_map(|px| {
            px[..3]
                .iter()
                .map(|c| (c.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8)
        })
        .collect();
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("nested_vector_x3.png");
    let file = std::fs::File::create(&path).expect("PNG を作れる");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), size.width, size.height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .expect("ヘッダー")
        .write_image_data(&rgb)
        .expect("画素");
    eprintln!("入れ子 3 倍の画像: {}", path.display());
}
