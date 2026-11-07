//! フレームのキャッシュ。同じ画像は描き直さず、変わった所だけを描き直し、追い出した画像はディスクから戻る。
//! GPU が無い環境ではスキップし、`MM_REQUIRE_GPU` があれば失敗にする。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use mm_cache_api::CacheStore;
use mm_doc::{Composition, Layer, Node, Project};
use mm_node_api::{FrameCx, FrameRef, NodeKind, Quality, Traits};
use mm_nodes_basic::all_kinds;
use mm_render::{FrameCacheConfig, GpuContext, GpuRequest, WgpuRenderer};
use mm_render_api::{RenderRequest, Renderer};
use mm_testkit::audio::SampleMixerFactory;
use mm_testkit::cache::MemoryCache;
use mm_testkit::decode::SampleDecoderFactory;
use mm_testkit::fixtures::sample_services;
use mm_testkit::node::MapRegistry;
use mm_types::{
    Id, ParamSchema, ParamSpec, ParamType, ParamValue, Params, Rational, RationalTime, Result,
    Rgba, RgbaImage, Size,
};

const SIZE: Size = Size::new(64, 36);

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

/// 描いた回数を数える種類。`color` の赤に時刻を足して塗る（時間で変わる）。
struct Counting {
    renders: Arc<AtomicUsize>,
    deterministic: bool,
}

impl NodeKind for Counting {
    fn id(&self) -> &'static str {
        "counting"
    }

    fn version(&self) -> u32 {
        1
    }

    fn params(&self) -> ParamSchema {
        ParamSchema::new(vec![ParamSpec::new(
            "shade",
            ParamType::Number {
                min: None,
                max: None,
            },
            ParamValue::Number(0.0),
        )])
    }

    fn traits(&self) -> Traits {
        Traits {
            deterministic: self.deterministic,
            ..Traits::PIXEL
        }
    }

    fn migrate(&self, _old: serde_json::Value, _from: u32) -> Result<Params> {
        Ok(Params::default())
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        self.renders.fetch_add(1, Ordering::SeqCst);
        let shade = cx.params.number_or("shade", 0.0) as f32;
        let t = cx.time.num() as f32 / cx.time.den() as f32;
        let color = Rgba::new(shade, (t * 0.25).min(1.0), 0.0, 1.0);
        Ok(FrameRef::cpu(RgbaImage::filled(cx.size, color)))
    }
}

struct Fixture {
    renders: Arc<AtomicUsize>,
    nodes: MapRegistry,
    renderer: WgpuRenderer,
}

fn fixture(ctx: &Arc<GpuContext>, deterministic: bool, config: FrameCacheConfig) -> Fixture {
    let renders = Arc::new(AtomicUsize::new(0));
    let mut nodes = MapRegistry::default();
    for kind in all_kinds() {
        nodes.insert(kind);
    }
    nodes.insert(Arc::new(Counting {
        renders: renders.clone(),
        deterministic,
    }));
    let renderer = WgpuRenderer::new(
        ctx.clone(),
        Arc::new(SampleDecoderFactory::default()),
        Arc::new(SampleMixerFactory),
        sample_services(),
    )
    .with_frame_cache(config);
    Fixture {
        renders,
        nodes,
        renderer,
    }
}

fn counting(layer: &str, shade: f64) -> Layer {
    let node = Node::new("counting", 1).with_param("shade", ParamValue::Number(shade));
    Layer::new(
        layer,
        node,
        RationalTime::ZERO,
        RationalTime::ZERO,
        RationalTime::seconds(10),
    )
}

fn nested(layer: &str, comp: &str) -> Layer {
    let node = Node::new("composition", 1).with_param("composition", ParamValue::Text(comp.into()));
    Layer::new(
        layer,
        node,
        RationalTime::ZERO,
        RationalTime::ZERO,
        RationalTime::seconds(10),
    )
}

fn comp(id: &str, layers: Vec<Layer>) -> Composition {
    Composition::new(
        Id::new(id),
        SIZE.width,
        SIZE.height,
        Rational::integer(30),
        RationalTime::seconds(10),
    )
    .with_layers(layers)
}

fn project(comps: Vec<Composition>) -> Project {
    let mut p = Project::new();
    p.compositions = comps;
    p
}

fn at(frame: i64) -> RationalTime {
    RationalTime::from_frames(frame, Rational::integer(30)).expect("表せる")
}

fn render(f: &mut Fixture, p: &Project, frame: i64) -> Arc<RgbaImage> {
    let id = Id::new("main");
    let image = f
        .renderer
        .render_frame(&RenderRequest {
            project: p,
            composition: &id,
            time: at(frame),
            size: SIZE,
            quality: Quality::Full,
            nodes: &f.nodes,
        })
        .expect("描ける");
    f.renderer
        .gpu()
        .download(&image)
        .expect("リードバックできる")
}

fn renders(f: &Fixture) -> usize {
    f.renders.load(Ordering::SeqCst)
}

#[test]
fn 同じフレームの_2_回目は種類を呼ばず_同じ画像() {
    let Some(ctx) = gpu() else { return };
    let mut f = fixture(&ctx, true, FrameCacheConfig::default());
    let p = project(vec![comp("main", vec![counting("a", 0.5)])]);
    let first = render(&mut f, &p, 3);
    let second = render(&mut f, &p, 3);
    assert_eq!(renders(&f), 1);
    assert_eq!(*first, *second);
    let stats = f.renderer.frame_cache_stats();
    assert_eq!((stats.lookups, stats.hits), (2, 1));
    render(&mut f, &p, 4);
    assert_eq!(renders(&f), 2, "別のフレームは描く");
}

#[test]
fn パラメータを変えたレイヤーだけ描き直す() {
    let Some(ctx) = gpu() else { return };
    let mut f = fixture(&ctx, true, FrameCacheConfig::default());
    let mut p = project(vec![comp(
        "main",
        vec![counting("a", 0.5), counting("b", 0.25)],
    )]);
    render(&mut f, &p, 0);
    assert_eq!(renders(&f), 2);
    p.compositions[0].layers[0] = counting("a", 0.75);
    render(&mut f, &p, 0);
    assert_eq!(renders(&f), 3, "変えた a だけ描く");
}

#[test]
fn 触ったレイヤーに頼った画像だけ捨てる() {
    let Some(ctx) = gpu() else { return };
    let mut f = fixture(&ctx, true, FrameCacheConfig::default());
    let p = project(vec![comp(
        "main",
        vec![counting("a", 0.5), counting("b", 0.25)],
    )]);
    render(&mut f, &p, 0);
    assert_eq!(f.renderer.frame_cache_stats().entries, 2);
    f.renderer.invalidate_touched(&["a".to_string()]);
    render(&mut f, &p, 0);
    assert_eq!(renders(&f), 3, "a だけ描き直す");
    assert_eq!(f.renderer.frame_cache_stats().entries, 2);
}

#[test]
fn 入れ子の中を変えると入れ子の画像を描き直す() {
    let Some(ctx) = gpu() else { return };
    let mut f = fixture(&ctx, true, FrameCacheConfig::default());
    let mut p = project(vec![
        comp("main", vec![counting("under", 0.1), nested("n", "inner")]),
        comp("inner", vec![counting("in", 0.5)]),
    ]);
    let before = render(&mut f, &p, 0);
    assert_eq!(renders(&f), 2);
    let again = render(&mut f, &p, 0);
    assert_eq!(renders(&f), 2, "入れ子も上のレイヤーも覚えている");
    assert_eq!(*before, *again);
    p.compositions[1].layers[0] = counting("in", 0.9);
    let after = render(&mut f, &p, 0);
    assert_eq!(renders(&f), 3, "入れ子の中だけ描き直す");
    assert_ne!(*before, *after, "編集が画像に出る");
    let entries = f.renderer.frame_cache_stats().entries;
    let range = mm_types::TimeRange::new(at(0), at(1)).unwrap();
    f.renderer.invalidate(&Id::new("inner"), range);
    assert!(
        f.renderer.frame_cache_stats().entries < entries,
        "入れ子の画像を捨てる"
    );
    render(&mut f, &p, 0);
    assert_eq!(renders(&f), 3, "中のレイヤーの画像は残るので種類は呼ばない");
}

#[test]
fn 決定的でない種類は覚えない() {
    let Some(ctx) = gpu() else { return };
    let mut f = fixture(&ctx, false, FrameCacheConfig::default());
    let p = project(vec![comp("main", vec![counting("a", 0.5)])]);
    render(&mut f, &p, 0);
    render(&mut f, &p, 0);
    assert_eq!(renders(&f), 2);
    assert_eq!(f.renderer.frame_cache_stats().entries, 0);
}

#[test]
fn 上限で追い出した画像はディスクから戻る() {
    let Some(ctx) = gpu() else { return };
    let store: Arc<dyn CacheStore> = Arc::new(MemoryCache::new(None));
    // 画像 1 枚分（64 x 36 x 8 バイト）だけ置ける。
    let one = (SIZE.width * SIZE.height * 8) as u64;
    let mut f = fixture(
        &ctx,
        true,
        FrameCacheConfig {
            memory_bytes: one,
            store: Some(store.clone()),
        },
    );
    let p = project(vec![comp("main", vec![counting("a", 0.5)])]);
    let first = render(&mut f, &p, 3);
    render(&mut f, &p, 3);
    render(&mut f, &p, 4);
    render(&mut f, &p, 5);
    f.renderer.wait_frame_cache_spills();
    assert!(store.size_bytes() > 0, "2 回使った画像は退避される");
    let before = renders(&f);
    let back = render(&mut f, &p, 3);
    assert_eq!(renders(&f), before, "ディスクから戻したので描かない");
    let diff = back.max_diff(&first).expect("同じ大きさ");
    assert!(diff <= 1e-6, "退避と復元で画素が変わらない: {diff}");
}

fn adjust(brightness: f64) -> mm_doc::Effect {
    mm_doc::Effect {
        id: Id::new("e"),
        node: Node::new("color.adjust", 1).with_param("brightness", ParamValue::Number(brightness)),
        enabled: true,
    }
}

fn sequence_layer(id: &str, name: &str) -> Layer {
    let node = Node::new("sequence", 1).with_param("sequence", ParamValue::Text(name.into()));
    Layer::new(
        id,
        node,
        RationalTime::ZERO,
        RationalTime::ZERO,
        RationalTime::seconds(10),
    )
}

fn sequence(shade: f64) -> mm_doc::Sequence {
    let node = Node::new("counting", 1).with_param("shade", ParamValue::Number(shade));
    mm_doc::Sequence {
        id: Id::new("s"),
        cuts: vec![mm_doc::Cut::new(
            "c",
            node,
            RationalTime::ZERO,
            RationalTime::seconds(5),
        )],
        origin: None,
    }
}

#[test]
fn 効果は覚えた画像を書き換えず_効果のパラメータで結果が変わる() {
    let Some(ctx) = gpu() else { return };
    let mut f = fixture(&ctx, true, FrameCacheConfig::default());
    let mut layer = counting("a", 0.25);
    layer.effects.push(adjust(1.0));
    let mut p = project(vec![comp("main", vec![layer])]);
    let first = render(&mut f, &p, 0);
    let second = render(&mut f, &p, 0);
    assert_eq!(renders(&f), 1, "効果の前の画像を覚えている");
    assert_eq!(*first, *second, "効果の出力が覚えた画像を壊さない");
    p.compositions[0].layers[0].effects[0] = adjust(0.0);
    let plain = render(&mut f, &p, 0);
    assert_eq!(
        renders(&f),
        1,
        "効果のパラメータだけの変更では種類を呼ばない"
    );
    assert_ne!(*first, *plain, "効果のパラメータが結果に出る");
}

#[test]
fn シーケンスのカットの画像を覚え_編集は触ったカットだけ描き直す() {
    let Some(ctx) = gpu() else { return };
    let mut f = fixture(&ctx, true, FrameCacheConfig::default());
    let mut p = project(vec![comp("main", vec![sequence_layer("seq", "s")])]);
    p.sequences = vec![sequence(0.5)];
    let first = render(&mut f, &p, 3);
    let second = render(&mut f, &p, 3);
    assert_eq!((renders(&f), *first == *second), (1, true));
    p.sequences = vec![sequence(0.75)];
    let edited = render(&mut f, &p, 3);
    assert_eq!(renders(&f), 2, "カットのパラメータが変わった");
    assert_ne!(*first, *edited);
    f.renderer.invalidate_touched(&["c".to_string()]);
    render(&mut f, &p, 3);
    assert_eq!(renders(&f), 3, "カットの ID に頼った画像を捨てた");
}

/// 入れ子を `start` 秒から 1 秒だけ置く（中の時刻は 0 から）。
fn nested_at(layer: &str, comp: &str, start: i64) -> Layer {
    Layer {
        start: RationalTime::seconds(start),
        out_point: RationalTime::seconds(1),
        ..nested(layer, comp)
    }
}

/// 同じ入れ子を順に並べると、2 つ目は 1 つ目と同じ中の時刻なので、中の種類を呼ばずに覚えた画像を使う
/// （素材なら、デコーダーが先頭へシークし直さない）。
#[test]
fn 同じ入れ子を順に並べた_2_つ目は覚えた画像を使う() {
    let Some(ctx) = gpu() else { return };
    let mut f = fixture(&ctx, true, FrameCacheConfig::default());
    let p = project(vec![
        comp(
            "main",
            vec![nested_at("s0", "scene", 0), nested_at("s1", "scene", 1)],
        ),
        comp("scene", vec![counting("a", 0.5)]),
    ]);
    let first: Vec<_> = (0..30).map(|i| render(&mut f, &p, i)).collect();
    assert_eq!(renders(&f), 30);
    let second: Vec<_> = (30..60).map(|i| render(&mut f, &p, i)).collect();
    assert_eq!(renders(&f), 30, "2 つ目の入れ子では中を描かない");
    assert!(first.iter().zip(&second).all(|(a, b)| **a == **b));
}

#[test]
fn 入れ子の中のシーケンスを編集すると入れ子の画像を描き直す() {
    let Some(ctx) = gpu() else { return };
    let mut f = fixture(&ctx, true, FrameCacheConfig::default());
    let mut p = project(vec![
        comp("main", vec![nested("n", "inner")]),
        comp("inner", vec![sequence_layer("seq", "s")]),
    ]);
    p.sequences = vec![sequence(0.5)];
    let before = render(&mut f, &p, 3);
    let again = render(&mut f, &p, 3);
    assert_eq!((renders(&f), *before == *again), (1, true));
    p.sequences = vec![sequence(0.75)];
    let after = render(&mut f, &p, 3);
    assert_ne!(
        *before, *after,
        "コンポジションの外のシーケンスの編集が入れ子の画像に出る"
    );
}
