//! 入れ子の画像のキーが、レイヤーの効果（種類が決定的か、効果が参照する素材の中身）を見ること。
//! GPU が無い環境ではスキップし、`MM_REQUIRE_GPU` があれば失敗にする。

use std::sync::Arc;

use mm_doc::{Composition, Effect, Layer, Node, Project};
use mm_node_api::{FrameCx, FrameRef, InputSchema, InputSpec, NodeKind, Quality, Traits};
use mm_nodes_basic::all_kinds;
use mm_render::{GpuContext, GpuRequest, WgpuRenderer};
use mm_render_api::{RenderRequest, Renderer};
use mm_testkit::audio::SampleMixerFactory;
use mm_testkit::decode::SampleDecoderFactory;
use mm_testkit::fixtures::{invert_lut_asset, sample_services, INVERT_LUT_ID};
use mm_testkit::node::MapRegistry;
use mm_types::{Id, ParamSchema, ParamValue, Params, Rational, RationalTime, Result, Rgba, Size};

const SIZE: Size = Size::new(32, 18);

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

/// 入力をそのまま返す、決定的でない効果。
struct Noisy;

impl NodeKind for Noisy {
    fn id(&self) -> &'static str {
        "noisy"
    }

    fn version(&self) -> u32 {
        1
    }

    fn params(&self) -> ParamSchema {
        ParamSchema::new(vec![])
    }

    fn inputs(&self) -> InputSchema {
        InputSchema {
            inputs: vec![InputSpec {
                name: "source".into(),
                optional: false,
            }],
        }
    }

    fn traits(&self) -> Traits {
        Traits {
            deterministic: false,
            ..Traits::PIXEL
        }
    }

    fn migrate(&self, _old: serde_json::Value, _from: u32) -> Result<Params> {
        Ok(Params::default())
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        let t = cx.time;
        cx.inputs.frame(0, t)
    }
}

fn registry() -> MapRegistry {
    let mut r = MapRegistry::default();
    for kind in all_kinds() {
        r.insert(kind);
    }
    r.insert(Arc::new(Noisy));
    r
}

fn renderer(ctx: &Arc<GpuContext>) -> WgpuRenderer {
    WgpuRenderer::new(
        ctx.clone(),
        Arc::new(SampleDecoderFactory::default()),
        Arc::new(SampleMixerFactory),
        sample_services(),
    )
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

/// 単色に効果 `node` を掛けた入れ子を、親に置いたプロジェクト。
fn nested_with_effect(node: Node) -> Project {
    let solid =
        Node::new("solid", 1).with_param("color", ParamValue::Color(Rgba::new(0.2, 0.4, 0.6, 1.0)));
    let mut inner_layer = layer("inner-solid", solid);
    inner_layer.effects.push(Effect {
        id: Id::new("fx"),
        node,
        enabled: true,
    });
    let nested =
        Node::new("composition", 1).with_param("composition", ParamValue::Text("inner".into()));
    let mut p = Project::new();
    p.compositions = vec![
        comp("inner", vec![inner_layer]),
        comp("main", vec![layer("n", nested)]),
    ];
    p.assets.push(invert_lut_asset());
    p
}

fn lut() -> Node {
    Node::new("color.lut", 1).with_param("lut", ParamValue::AssetRef(Id::new(INVERT_LUT_ID)))
}

fn render(r: &mut WgpuRenderer, p: &Project) {
    let nodes = registry();
    r.render_frame(&RenderRequest {
        project: p,
        composition: &Id::new("main"),
        time: RationalTime::ZERO,
        size: SIZE,
        quality: Quality::Full,
        nodes: &nodes,
    })
    .expect("描ける");
    assert!(r.take_errors().is_empty());
}

fn entries(r: &WgpuRenderer) -> usize {
    r.frame_cache_stats().entries
}

#[test]
fn 入れ子の中の効果が決定的でなければ入れ子を覚えない() {
    let Some(ctx) = gpu() else { return };
    let mut r = renderer(&ctx);
    render(&mut r, &nested_with_effect(Node::new("noisy", 1)));
    assert_eq!(entries(&r), 1, "中の単色だけを覚え、入れ子は覚えない");
}

#[test]
fn 入れ子の中の効果の素材の中身が変わったら入れ子を描き直す() {
    let Some(ctx) = gpu() else { return };
    let mut r = renderer(&ctx);
    let mut p = nested_with_effect(lut());
    render(&mut r, &p);
    assert_eq!(entries(&r), 2, "中の単色と入れ子");
    render(&mut r, &p);
    assert_eq!(entries(&r), 2, "同じ中身なら覚えた入れ子に当たる");
    p.assets[0].content_hash = "invertcube000001".into();
    render(&mut r, &p);
    assert_eq!(entries(&r), 3, "LUT の中身が変わった入れ子は別のキー");
}

#[test]
fn 取り込み前の素材に頼る入れ子は覚えない() {
    let Some(ctx) = gpu() else { return };
    let mut r = renderer(&ctx);
    let mut p = nested_with_effect(lut());
    p.assets[0].content_hash.clear();
    render(&mut r, &p);
    assert_eq!(entries(&r), 1, "contentHash が空の素材は中身で引けない");
}

#[test]
fn 効果の素材を触ると入れ子の画像を捨てる() {
    let Some(ctx) = gpu() else { return };
    let mut r = renderer(&ctx);
    let p = nested_with_effect(lut());
    render(&mut r, &p);
    assert_eq!(entries(&r), 2);
    r.invalidate_touched(&[INVERT_LUT_ID.to_owned()]);
    assert_eq!(entries(&r), 1, "LUT に頼った入れ子だけ捨てる");
}
