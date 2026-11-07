//! GPU の時間が `mm-trace` に載ること。購読者は `with_default` でスレッドごとに置くので、同じプロセスの他のテストと競合しない。

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
const FRAMES: u32 = 8;

fn project() -> Project {
    let layer = Layer::new(
        "a",
        Node::new("solid", 1).with_param("color", ParamValue::Color(Rgba::WHITE)),
        RationalTime::ZERO,
        RationalTime::ZERO,
        RationalTime::seconds(2),
    );
    let mut p = Project::new();
    p.compositions.push(
        Composition::new(
            Id::new("main"),
            SIZE.width,
            SIZE.height,
            Rational::integer(30),
            RationalTime::seconds(2),
        )
        .with_layers(vec![layer]),
    );
    p
}

/// 購読者が 1 つだけのとき、tracing は新しい呼び出し箇所の関心を「今のスレッドの購読者」で決めて
/// プロセス全体に覚える。購読者の無い別のテストのスレッドが先に踏むと「関心なし」で固まり、
/// このテストのスパンが落ちる。何もしない購読者を全体の既定に 1 つ置き、常に全購読者で決めさせる。
fn keep_callsite_interest_global() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        tracing::subscriber::set_global_default(tracing_subscriber::registry())
            .expect("この実行ファイルで全体の購読者を置くのはここだけ");
    });
}

#[test]
fn gpu_の時間が_mm_trace_に載る() {
    keep_callsite_interest_global();
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
        for _ in 0..FRAMES {
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
        // 通常の描画は GPU の読み取りを待たないので、負荷で遅れても結果が決まるよう待って載せる。
        r.flush_gpu_time();
    });
    let snap = handle.snapshot();
    assert_eq!(snap.spans["render.frame"].count, u64::from(FRAMES));
    assert!(snap.gauges["render.gpu_frame_us"] >= 0.0);
    assert!(
        snap.spans["render.gpu_frame"].count >= 2,
        "読み取りの空きがあった分のフレームが載る"
    );
}
