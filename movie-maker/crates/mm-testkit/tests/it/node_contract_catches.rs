//! 種類の契約が、契約を破る種類を検出することの確認。

use std::sync::Arc;

use mm_node_api::{FrameCx, FrameRef, NodeKind, Traits};
use mm_testkit::node::{contract, NodeFixture};
use mm_types::{ParamSchema, Params, Rect, Result, Rgba, RgbaImage, Size};

/// 入力 0 を返す効果。`lie` が真なら描画は入力と違う画像を返すのに `is_identity` を真にする。
struct InputAware {
    lie: bool,
}

impl NodeKind for InputAware {
    fn id(&self) -> &'static str {
        "input-aware"
    }

    fn version(&self) -> u32 {
        1
    }

    fn params(&self) -> ParamSchema {
        ParamSchema::new(vec![])
    }

    fn traits(&self) -> Traits {
        Traits::PIXEL
    }

    fn migrate(&self, old: serde_json::Value, _from: u32) -> Result<Params> {
        Ok(serde_json::from_value(old)?)
    }

    fn is_identity(&self, cx: &FrameCx<'_>) -> bool {
        !cx.inputs.is_empty()
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        if self.lie || cx.inputs.is_empty() {
            return Ok(FrameRef::cpu(RgbaImage::filled(cx.size, Rgba::BLACK)));
        }
        cx.inputs.frame(0, cx.time)
    }
}

fn with_input() -> NodeFixture {
    NodeFixture {
        inputs: vec![Arc::new(RgbaImage::filled(
            Size::new(64, 36),
            Rgba::new(0.2, 0.4, 0.6, 1.0),
        ))],
        ..NodeFixture::default()
    }
}

#[test]
fn 入力を見て_is_identity_を決める種類は正直なら通る() {
    contract(|| Arc::new(InputAware { lie: false }), &with_input());
}

#[test]
#[should_panic(expected = "is_identity なら入力と同じ")]
fn 入力を見て_is_identity_を決める種類の嘘を見つける() {
    contract(|| Arc::new(InputAware { lie: true }), &with_input());
}

/// 左上に固定の大きさの四角を描く。`scale` を無視するのに `vector` を名乗る。
struct IgnoresScale;

impl NodeKind for IgnoresScale {
    fn id(&self) -> &'static str {
        "ignores-scale"
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

    fn migrate(&self, old: serde_json::Value, _from: u32) -> Result<Params> {
        Ok(serde_json::from_value(old)?)
    }

    fn region_of_definition(&self, cx: &FrameCx<'_>) -> Rect {
        Rect::new(0.0, 0.0, 10.0 * cx.scale, 10.0 * cx.scale)
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        let mut img = RgbaImage::transparent(cx.size);
        img.fill_rect(Rect::new(0.0, 0.0, 10.0, 10.0), Rgba::WHITE);
        Ok(FrameRef::cpu(img))
    }
}

#[test]
#[should_panic(expected = "ベクターの縁は 2 倍の位置")]
fn ベクターを名乗って倍率を無視する種類を見つける() {
    contract(|| Arc::new(IgnoresScale), &NodeFixture::default());
}
