//! 種類のテストと期待画像のテストが共有するハーネス。GPU が無い環境ではスキップし、`MM_REQUIRE_GPU` があれば失敗にする。

use std::sync::Arc;

use mm_color_lut::LutColor;
use mm_doc::{Composition, Effect, Layer, Node, Project, Sequence, Transition};
use mm_node_api::{Quality, Services};
use mm_nodes_basic::all_kinds;
use mm_render::{GpuContext, GpuRequest, WgpuRenderer};
use mm_render_api::{RenderRequest, Renderer};
use mm_testkit::audio::SampleMixerFactory;
use mm_testkit::decode::SampleDecoderFactory;
use mm_testkit::fixtures::sample_services;
use mm_testkit::node::MapRegistry;
use mm_types::{Id, ParamValue, Rational, RationalTime, Rgba, RgbaImage, Size};

pub fn gpu() -> Option<Arc<GpuContext>> {
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

pub fn registry() -> MapRegistry {
    let mut r = MapRegistry::default();
    for kind in all_kinds() {
        r.insert(kind);
    }
    r
}

/// 実際の色の変換（`LutColor`）と、モックの文字。
pub fn services() -> Services {
    Services {
        color: Arc::new(LutColor),
        ..sample_services()
    }
}

pub fn renderer(ctx: &Arc<GpuContext>) -> WgpuRenderer {
    WgpuRenderer::new(
        ctx.clone(),
        Arc::new(SampleDecoderFactory::default()),
        Arc::new(SampleMixerFactory),
        services(),
    )
}

pub fn seconds(s: i64) -> RationalTime {
    RationalTime::seconds(s)
}

/// 秒 0 から `len` 秒のレイヤー。
pub fn layer(id: &str, source: Node, len: i64) -> Layer {
    Layer::new(id, source, seconds(0), seconds(0), seconds(len))
}

pub fn solid(color: Rgba) -> Node {
    Node::new("solid", 1).with_param("color", ParamValue::Color(color))
}

pub fn shader(code: &str) -> Node {
    Node::new("shader.wgsl", 1).with_param("code", ParamValue::Text(code.into()))
}

pub fn effect(id: &str, node: Node) -> Effect {
    Effect {
        id: Id::new(id),
        node,
        enabled: true,
    }
}

pub fn comp(id: &str, size: Size, len: i64, layers: Vec<Layer>) -> Composition {
    Composition::new(
        Id::new(id),
        size.width,
        size.height,
        Rational::integer(30),
        seconds(len),
    )
    .with_layers(layers)
}

pub fn project(comps: Vec<Composition>, sequences: Vec<Sequence>) -> Project {
    let mut p = Project::new();
    p.compositions = comps;
    p.sequences = sequences;
    p
}

/// カットの境のクロスディゾルブ。
pub fn dissolve(len: RationalTime) -> Transition {
    Transition {
        node: Node::new("transition.dissolve", 1),
        duration: len,
    }
}

/// コンポジション `name` の時刻 `t` を描いて CPU の画像にする。
pub fn render_at(
    r: &mut WgpuRenderer,
    p: &Project,
    name: &str,
    t: RationalTime,
    size: Size,
) -> Arc<RgbaImage> {
    let id = Id::new(name);
    let nodes = registry();
    let frame = r
        .render_frame(&RenderRequest {
            project: p,
            composition: &id,
            time: t,
            size,
            quality: Quality::Full,
            nodes: &nodes,
        })
        .expect("描ける");
    r.gpu().download(&frame).expect("リードバックできる")
}
