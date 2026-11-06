//! GPU の時間が `mm-trace` に載ること。購読者の登録が他のテストと競合しないよう、1 ファイル 1 テストにする。

use std::sync::Arc;

use mm_doc::{Composition, Layer, Node, Project};
use mm_node_api::Quality;
use mm_nodes_basic::all_kinds;
use mm_render::{GpuContext, GpuRequest, WgpuRenderer};
use mm_render_api::{RenderRequest, Renderer};
use mm_testkit::audio::SampleMixerFactory;
use mm_testkit::decode::SampleDecoderFactory;
use mm_testkit::fixtures::sample_services;
use mm_testkit::node::MapRegistry;
use mm_types::{Id, ParamValue, Rational, RationalTime, Rgba, Size};
use tracing_subscriber::prelude::*;

const SIZE: Size = Size::new(64, 36);

fn project() -> Project {
    let layer = Layer::new(
        "a",
        Node::new("solid", 1).with_param("color", ParamValue::Color(Rgba::WHITE)),
        RationalTime::ZERO,
        RationalTime::ZERO,
        RationalTime::seconds(2),
    );
    let mut p = Project::new();
    p.compositions.push(Composition {
        id: Id::new("main"),
        width: SIZE.width,
        height: SIZE.height,
        fps: Rational::integer(30),
        duration: RationalTime::seconds(2),
        layers: vec![layer],
        markers: Vec::new(),
    });
    p
}

#[test]
fn gpu_の時間が_mm_trace_に載る() {
    let ctx = match GpuContext::shared(&GpuRequest::default()) {
        Ok(ctx) => ctx,
        Err(e) if std::env::var_os("MM_REQUIRE_GPU").is_some() => {
            panic!("MM_REQUIRE_GPU が有るのに GPU を作れない: {e}")
        }
        Err(e) => {
            eprintln!("GPU が無いのでスキップする: {e}");
            return;
        }
    };
    if !ctx.timestamps {
        eprintln!("このアダプターにはタイムスタンプが無いのでスキップする");
        return;
    }
    let mut nodes = MapRegistry::default();
    for kind in all_kinds() {
        nodes.insert(kind);
    }
    let project = project();
    let id = Id::new("main");
    let (layer, handle) = mm_trace::MetricsLayer::new();
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, || {
        let mut r = WgpuRenderer::new(
            Arc::clone(&ctx),
            Arc::new(SampleDecoderFactory::default()),
            Arc::new(SampleMixerFactory),
            sample_services(),
        );
        for _ in 0..3 {
            r.render_frame(&RenderRequest {
                project: &project,
                composition: &id,
                time: RationalTime::ZERO,
                size: SIZE,
                quality: Quality::Full,
                nodes: &nodes,
            })
            .expect("描ける");
        }
    });
    let snap = handle.snapshot();
    assert_eq!(snap.spans["render.frame"].count, 3);
    assert!(snap.gauges["render.gpu_frame_us"] >= 0.0);
    assert!(
        snap.spans["render.gpu_frame"].count >= 2,
        "2 フレーム目から前のフレームの分が出る"
    );
}
