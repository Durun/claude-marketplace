//! 描画器の契約のうち、文書の見本とは別に自前の文書を組んで確かめる項目。
//! レイヤーの変形、失敗したレイヤー、コマの寿命。

use std::sync::Arc;

use mm_doc::{Composition, Layer, Node, Param, Project, Transform};
use mm_node_api::Quality;
use mm_render_api::{RenderRequest, Renderer, KEPT_RENDERED_FRAMES};
use mm_types::{CompositionId, Id, ParamValue, RationalTime, Rgba, RgbaImage};

use crate::render::RenderFixture;

const CASE_COMPOSITION: &str = "mm-contract-cases";

/// 直近 `KEPT_RENDERED_FRAMES` 回の `render_frame` が返したコマは読み戻せる。
pub(crate) fn check_frame_lifetime(r: &mut dyn Renderer, req: &RenderRequest<'_>) {
    let first = r.render_frame(req).expect("描ける");
    for _ in 1..KEPT_RENDERED_FRAMES {
        let later = r.render_frame(req).expect("描ける");
        r.gpu().download(&later).expect("新しいコマは読み戻せる");
    }
    let oldest = r
        .gpu()
        .download(&first)
        .expect("直近 KEPT_RENDERED_FRAMES 回のうち最も古いコマも有効");
    assert_eq!(oldest.size(), req.size, "寿命の内側のコマは中身も壊れない");
}

fn layer(kind: &str, version: u32, transform: Transform) -> Layer {
    Layer {
        id: Id::new(format!("case-{kind}")),
        source: Node::new(kind, version),
        start: RationalTime::ZERO,
        in_point: RationalTime::ZERO,
        out_point: RationalTime::seconds(1),
        opacity: Param::constant(ParamValue::Number(1.0)),
        transform,
        audio: None,
        lock: false,
    }
}

fn render_layers(
    r: &mut dyn Renderer,
    fx: &RenderFixture<'_>,
    layers: Vec<Layer>,
) -> Arc<RgbaImage> {
    let base = fx.project.composition(fx.composition).expect("ある");
    let mut project: Project = fx.project.clone();
    let id: CompositionId = Id::new(CASE_COMPOSITION);
    project.compositions.push(Composition {
        id: id.clone(),
        width: fx.size.width,
        height: fx.size.height,
        fps: base.fps,
        duration: RationalTime::seconds(1),
        layers,
    });
    let frame = r
        .render_frame(&RenderRequest {
            project: &project,
            composition: &id,
            time: RationalTime::ZERO,
            size: fx.size,
            quality: Quality::Full,
            nodes: fx.nodes,
        })
        .expect("失敗したレイヤーがあっても、フレーム全体は描ける");
    r.gpu().download(&frame).expect("読み戻し")
}

fn alpha(img: &RgbaImage, x: u32, y: u32) -> f32 {
    img.pixel(x, y).a
}

/// 位置と拡大がコンポジションの画素で効くこと、`Err` のレイヤーだけが透明で残りは描けること。
/// `nodes` に `solid`（画面全体を `color` で塗る種類）が要る。大きさは 8x8 画素以上の偶数。
pub(crate) fn check_transform_and_failed_layer(r: &mut dyn Renderer, fx: &RenderFixture<'_>) {
    let (w, h) = (fx.size.width, fx.size.height);
    assert!(w >= 8 && h >= 8 && w % 2 == 0 && h % 2 == 0, "契約の大きさ");
    let solid = fx.nodes.get("solid").expect("契約には solid の種類が要る");
    let version = solid.version();
    let solid_layer = |transform: Transform| {
        let mut l = layer("solid", version, transform);
        l.source = l.source.with_param("color", ParamValue::Color(Rgba::WHITE));
        l
    };

    let shifted = Transform {
        position: Param::constant(ParamValue::Vec2([f64::from(w / 2), 0.0])),
        ..Transform::default()
    };
    let img = render_layers(r, fx, vec![solid_layer(shifted)]);
    assert_eq!(alpha(&img, 1, h / 2), 0.0, "右へ半分ずらすと左半分は空く");
    assert_eq!(alpha(&img, w - 2, h / 2), 1.0, "ずらした先は塗られる");

    let shrunk = Transform {
        scale: Param::constant(ParamValue::Number(0.5)),
        ..Transform::default()
    };
    let img = render_layers(r, fx, vec![solid_layer(shrunk)]);
    assert_eq!(
        alpha(&img, w / 2, h / 2),
        1.0,
        "半分に縮めても中心は塗られる"
    );
    assert_eq!(alpha(&img, 0, 0), 0.0, "半分に縮めると隅は空く");

    let broken = layer("no-such-kind", 1, Transform::default());
    let img = render_layers(r, fx, vec![solid_layer(Transform::default()), broken]);
    assert_eq!(
        alpha(&img, 1, 1),
        1.0,
        "失敗したレイヤーだけが透明で下は描かれる"
    );
}
