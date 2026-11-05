//! 見本のプロジェクトと、見本の口の束。

use std::path::Path;
use std::sync::Arc;

use mm_doc::{
    Asset, Composition, Layer, LayerAudio, Node, Param, Project, RenderTarget, Transform,
};
use mm_node_api::Services;
use mm_types::{
    AssetId, ColorSpace, CompositionId, Id, ParamValue, Rational, RationalTime, RenderTargetId,
    Rgba,
};

use crate::color::SampleColor;
use crate::decode::SampleMedia;
use crate::node::SAMPLE_KIND_VERSION;
use crate::text::{SampleRasterizer, SampleShaper};

/// 見本の素材のパス。見本のデコーダーはパスを見ないので何でもよい。
pub const SAMPLE_ASSET_PATH: &str = "fixtures/media/sample.mp4";
/// 縦の 1 本で素材を切る入りの時刻（秒）。
pub const VERTICAL_IN_SECONDS: i64 = 3;
/// 縦の 1 本のコンポジションの長さ（秒）。
pub const VERTICAL_DURATION_SECONDS: i64 = 2;
/// 縦の 1 本で載せる文字。
pub const VERTICAL_TEXT: &str = "Hello";

/// 見本の口の束。
pub fn sample_services() -> Services {
    Services {
        text_shaper: Arc::new(SampleShaper),
        text_rasterizer: Arc::new(SampleRasterizer),
        color: Arc::new(SampleColor),
    }
}

/// 縦の 1 本のプロジェクト: 素材 1 本を切って、文字を載せ、音を付けて、`output_path` に書き出す。
pub fn vertical_project(
    media: &SampleMedia,
    output_path: &Path,
) -> (Project, CompositionId, RenderTargetId) {
    let comp_id: CompositionId = Id::new("main");
    let target_id: RenderTargetId = Id::new("mp4");
    let asset_id: AssetId = Id::new("clip");
    let duration = RationalTime::seconds(VERTICAL_DURATION_SECONDS);
    let in_point = RationalTime::seconds(VERTICAL_IN_SECONDS);

    let footage = footage_layer(&asset_id, in_point, duration);
    let text = text_layer(duration);

    let mut project = Project::new();
    project.assets.push(Asset {
        id: asset_id,
        path: SAMPLE_ASSET_PATH.into(),
        content_hash: "sample0000000000".into(),
        color_space: ColorSpace::bt709(),
        duration: media.duration,
        fps: Some(media.fps),
        timebase_offset: RationalTime::ZERO,
    });
    project.compositions.push(Composition {
        id: comp_id.clone(),
        width: media.size.width,
        height: media.size.height,
        fps: media.fps,
        duration,
        layers: vec![footage, text],
    });
    project.render_targets.push(RenderTarget {
        id: target_id.clone(),
        composition: comp_id.clone(),
        width: media.size.width,
        height: media.size.height,
        fps: media.fps,
        codec: crate::SAMPLE_ID.into(),
        output_path: output_path.to_string_lossy().into_owned(),
        loudness_lufs: None,
    });
    (project, comp_id, target_id)
}

/// 素材を `in_point` から `duration` だけ切り、音を `voice` の役割で付けたレイヤー。
fn footage_layer(asset_id: &AssetId, in_point: RationalTime, duration: RationalTime) -> Layer {
    Layer {
        id: Id::new("l-footage"),
        source: Node::new("footage", SAMPLE_KIND_VERSION)
            .with_param("asset", ParamValue::AssetRef(asset_id.clone())),
        start: RationalTime::ZERO,
        in_point,
        out_point: in_point.checked_add(duration).expect("見本の長さは表せる"),
        opacity: Param::constant(ParamValue::Number(1.0)),
        transform: Transform::default(),
        audio: Some(LayerAudio {
            role: "voice".into(),
            gain_db: Param::constant(ParamValue::Number(0.0)),
            pan: Param::constant(ParamValue::Number(0.0)),
        }),
        lock: false,
    }
}

/// `VERTICAL_TEXT` を左下に載せるレイヤー。
fn text_layer(duration: RationalTime) -> Layer {
    Layer {
        id: Id::new("l-text"),
        source: Node::new("text", SAMPLE_KIND_VERSION)
            .with_param("text", ParamValue::Text(VERTICAL_TEXT.into()))
            .with_param("size", ParamValue::Number(24.0))
            .with_param("position", ParamValue::Vec2([16.0, 120.0]))
            .with_param("color", ParamValue::Color(Rgba::WHITE)),
        start: RationalTime::ZERO,
        in_point: RationalTime::ZERO,
        out_point: duration,
        opacity: Param::constant(ParamValue::Number(1.0)),
        transform: Transform::default(),
        audio: None,
        lock: false,
    }
}

/// 縦の 1 本の既定の fps。
pub fn vertical_fps() -> Rational {
    SampleMedia::default().fps
}
