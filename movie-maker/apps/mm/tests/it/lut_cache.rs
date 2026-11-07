//! `mm ingest` が取り込んだ `.cube` の素材（`contentHash` つき）を使う入れ子の画像が、フレームのキャッシュに当たること。
//! 中身を書き換えて取り込み直すと別の素材になり、別のキーで描き直すこと。GPU が無い環境ではスキップし、
//! `MM_REQUIRE_GPU` があれば失敗にする。

use std::path::Path;
use std::sync::{Arc, OnceLock};

use mm_doc::{Asset, Composition, Effect, Layer, Node, Project};
use mm_ingest::{CacheLayout, IngestOptions, PreviewEncoder, Tools};
use mm_nodes_basic::all_kinds;
use mm_render::{GpuContext, GpuRequest, WgpuRenderer};
use mm_render_api::{RenderRequest, Renderer};
use mm_testkit::audio::SampleMixerFactory;
use mm_testkit::decode::SampleDecoderFactory;
use mm_testkit::fixtures::sample_services;
use mm_testkit::node::MapRegistry;
use mm_types::{Id, ParamValue, Rational, RationalTime, Rgba, Size};

const SIZE: Size = Size::new(32, 18);
const INVERT_CUBE: &str = include_str!("../../../../fixtures/luts/invert.cube");

/// 自前のコンテキスト。共有の GPU を既定の頼み方で先に作ると、同じプロセスで D3D12 を頼む書き出しの試験が
/// 共有の GPU を使えなくなる。共有の GPU と同じく、プロセスが終わるまでデバイスを捨てない。
fn gpu() -> Option<Arc<GpuContext>> {
    static CTX: OnceLock<Result<Arc<GpuContext>, String>> = OnceLock::new();
    let ctx =
        CTX.get_or_init(|| GpuContext::with(&GpuRequest::default()).map_err(|e| e.to_string()));
    match ctx {
        Ok(ctx) => Some(ctx.clone()),
        Err(e) if std::env::var_os("MM_REQUIRE_GPU").is_some() => {
            panic!("MM_REQUIRE_GPU が有るのに GPU を作れない: {e}")
        }
        Err(e) => {
            eprintln!("GPU が無いのでスキップする: {e}");
            None
        }
    }
}

fn ingest_lut(dir: &Path, cube: &str) -> Asset {
    let path = dir.join("look.cube");
    std::fs::write(&path, cube).unwrap();
    let opts = IngestOptions {
        cache: CacheLayout::new(dir.join("cache")),
        tools: Tools::default(),
        encoder: PreviewEncoder::Auto,
    };
    mm_ingest::ingest(&opts, &path).unwrap().asset
}

fn layer(id: &str, node: Node) -> Layer {
    Layer::new(
        id,
        node,
        RationalTime::ZERO,
        RationalTime::ZERO,
        RationalTime::seconds(2),
    )
}

fn comp(id: &str, layers: Vec<Layer>) -> Composition {
    Composition::new(
        Id::new(id),
        SIZE.width,
        SIZE.height,
        Rational::integer(30),
        RationalTime::seconds(2),
    )
    .with_layers(layers)
}

/// 単色に `color.lut`（素材 `lut`）を掛けた入れ子を、親に置いたプロジェクト。
fn project_with_lut(lut: Asset) -> Project {
    let solid =
        Node::new("solid", 1).with_param("color", ParamValue::Color(Rgba::new(0.2, 0.4, 0.6, 1.0)));
    let mut inner = layer("inner-solid", solid);
    inner.effects.push(Effect {
        id: Id::new("fx"),
        node: Node::new("color.lut", 1).with_param("lut", ParamValue::AssetRef(lut.id.clone())),
        enabled: true,
    });
    let nested =
        Node::new("composition", 1).with_param("composition", ParamValue::Text("inner".into()));
    let mut p = Project::new();
    p.compositions = vec![
        comp("inner", vec![inner]),
        comp("main", vec![layer("n", nested)]),
    ];
    p.assets.push(lut);
    p
}

fn render(r: &mut WgpuRenderer, p: &Project) {
    let mut nodes = MapRegistry::default();
    for kind in all_kinds() {
        nodes.insert(kind);
    }
    r.render_frame(&RenderRequest {
        project: p,
        composition: &Id::new("main"),
        time: RationalTime::ZERO,
        size: SIZE,
        quality: mm_node_api::Quality::Full,
        nodes: &nodes,
    })
    .expect("描ける");
    assert!(r.take_errors().is_empty());
}

#[test]
fn 取り込んだ_lut_を使う入れ子はキャッシュに当たり_中身が変われば描き直す() {
    let Some(ctx) = gpu() else { return };
    let mut r = WgpuRenderer::new(
        ctx,
        Arc::new(SampleDecoderFactory::default()),
        Arc::new(SampleMixerFactory),
        sample_services(),
    );
    let dir = tempfile::tempdir().unwrap();
    let lut = ingest_lut(dir.path(), INVERT_CUBE);
    assert!(
        !lut.content_hash.is_empty(),
        "取り込みで contentHash が入る"
    );

    let p = project_with_lut(lut);
    render(&mut r, &p);
    let first = r.frame_cache_stats();
    assert_eq!(first.entries, 2, "中の単色と入れ子をキャッシュする");
    render(&mut r, &p);
    let second = r.frame_cache_stats();
    assert_eq!(second.entries, 2, "同じ中身なら増えない");
    assert!(second.hits > first.hits, "入れ子がキャッシュに当たる");

    let changed = ingest_lut(
        dir.path(),
        &INVERT_CUBE.replace("TITLE \"invert\"", "TITLE \"x\""),
    );
    let p2 = project_with_lut(changed);
    render(&mut r, &p2);
    assert_eq!(r.frame_cache_stats().entries, 3, "別の素材は別のキー");
}
