//! フィクスチャのプロジェクトと、モックのインターフェースの一式。

use std::path::Path;
use std::sync::Arc;

use mm_doc::{Asset, Composition, Layer, LayerAudio, Node, Project, RenderTarget};
use mm_node_api::Services;
use mm_types::{
    AssetId, ColorSpace, CompositionId, Id, ParamValue, Rational, RationalTime, RenderTargetId,
    Rgba,
};

use crate::color::SampleColor;
use crate::decode::SampleMedia;
use crate::node::SAMPLE_KIND_VERSION;
use crate::text::{SampleRasterizer, SampleShaper};

/// フィクスチャの素材のパス。モックのデコーダーはパスを見ないので何でもよい。
pub const SAMPLE_ASSET_PATH: &str = "fixtures/media/sample.mp4";
/// 縦の 1 本で素材を切るイン点の時刻（秒）。
pub const VERTICAL_IN_SECONDS: i64 = 3;
/// 縦の 1 本のコンポジションの長さ（秒）。
pub const VERTICAL_DURATION_SECONDS: i64 = 2;
/// 縦の 1 本で載せる文字。
pub const VERTICAL_TEXT: &str = "Hello";
/// 色を反転する 3D LUT（`fixtures/luts/invert.cube`）の素材の ID。
pub const INVERT_LUT_ID: &str = "invert-lut";

/// 色を反転する 3D LUT の素材。`color.lut` の `lut` に `AssetRef(INVERT_LUT_ID)` で渡す。
pub fn invert_lut_asset() -> Asset {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/luts/invert.cube");
    Asset {
        id: Id::new(INVERT_LUT_ID),
        path: path.to_string_lossy().into_owned(),
        content_hash: "invertcube000000".into(),
        color_space: ColorSpace::working(),
        duration: RationalTime::ZERO,
        fps: None,
        timebase_offset: RationalTime::ZERO,
    }
}

/// モックのインターフェースの一式。
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
    project.compositions.push(
        Composition::new(
            comp_id.clone(),
            media.size.width,
            media.size.height,
            media.fps,
            duration,
        )
        .with_layers(vec![footage, text]),
    );
    project.render_targets.push(RenderTarget {
        id: target_id.clone(),
        composition: comp_id.clone(),
        width: media.size.width,
        height: media.size.height,
        fps: media.fps,
        codec: crate::SAMPLE_ID.into(),
        output_path: output_path.to_string_lossy().into_owned(),
        loudness_lufs: None,
        delivery: Default::default(),
    });
    (project, comp_id, target_id)
}

/// 素材を `in_point` から `duration` だけ切り、音を `voice` の役割で付けたレイヤー。
fn footage_layer(asset_id: &AssetId, in_point: RationalTime, duration: RationalTime) -> Layer {
    Layer {
        audio: Some(LayerAudio::new("voice")),
        ..Layer::new(
            "l-footage",
            Node::new("footage", SAMPLE_KIND_VERSION)
                .with_param("asset", ParamValue::AssetRef(asset_id.clone())),
            RationalTime::ZERO,
            in_point,
            in_point
                .checked_add(duration)
                .expect("フィクスチャの長さは表せる"),
        )
    }
}

/// `VERTICAL_TEXT` を左下に載せるレイヤー。
fn text_layer(duration: RationalTime) -> Layer {
    Layer::new(
        "l-text",
        Node::new("text", SAMPLE_KIND_VERSION)
            .with_param("text", ParamValue::Text(VERTICAL_TEXT.into()))
            .with_param("size", ParamValue::Number(24.0))
            .with_param("position", ParamValue::Vec2([16.0, 120.0]))
            .with_param("color", ParamValue::Color(Rgba::WHITE)),
        RationalTime::ZERO,
        RationalTime::ZERO,
        duration,
    )
}

/// 縦の 1 本の既定の fps。
pub fn vertical_fps() -> Rational {
    SampleMedia::default().fps
}
