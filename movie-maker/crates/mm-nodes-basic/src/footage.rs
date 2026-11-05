use mm_audio_api::AudioBlock;
use mm_node_api::{audio_frames_in, AudioCx, FrameCx, FrameRef, NodeKind, Traits};
use mm_types::{
    Id, MmError, ParamSchema, ParamSpec, ParamType, ParamValue, Params, RationalTime, Result,
};

use crate::util::{fit_contain, migrate_same_version};
use crate::KIND_VERSION;

fn asset_schema() -> ParamSchema {
    ParamSchema::new(vec![ParamSpec::new(
        "asset",
        ParamType::AssetRef,
        ParamValue::AssetRef(Id::new("")),
    )])
}

/// 素材の `t` のコマを画面に収めて返す。素材が無ければ透明。
fn render_source_frame(cx: &mut FrameCx<'_>, t: RationalTime) -> Result<FrameRef> {
    let size = cx.size;
    let Some(footage) = cx.footage.as_deref_mut() else {
        return Ok(FrameRef::Transparent { size });
    };
    let frame = footage.frame_at(t)?;
    let Some(image) = frame.image else {
        return Err(MmError::Unsupported(
            "GPU のコマはまだ受けない（CPU のコマだけ）".into(),
        ));
    };
    Ok(FrameRef::cpu(fit_contain(image, size)))
}

/// 撮影素材。`asset` の素材を描画器が開いて `cx.footage` に渡し、`cx.source_time` のコマを返す。
#[derive(Debug, Default)]
pub struct FootageKind;

impl NodeKind for FootageKind {
    fn id(&self) -> &'static str {
        "footage"
    }

    fn version(&self) -> u32 {
        KIND_VERSION
    }

    fn params(&self) -> ParamSchema {
        asset_schema()
    }

    fn traits(&self) -> Traits {
        Traits {
            audio: true,
            ..Traits::PIXEL
        }
    }

    fn migrate(&self, old: serde_json::Value, from: u32) -> Result<Params> {
        migrate_same_version(self, old, from)
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        let t = cx.source_time;
        render_source_frame(cx, t)
    }

    fn render_audio(&self, cx: &mut AudioCx<'_>) -> Result<AudioBlock> {
        match cx.footage.as_deref_mut() {
            Some(footage) => footage.audio_in(cx.source_range),
            None => Ok(AudioBlock::silence(
                cx.sample_rate,
                cx.channels,
                audio_frames_in(cx.range, cx.sample_rate),
            )),
        }
    }
}

/// 静止画。素材の最初のコマを使い、時間で変わらない。
#[derive(Debug, Default)]
pub struct ImageKind;

impl NodeKind for ImageKind {
    fn id(&self) -> &'static str {
        "image"
    }

    fn version(&self) -> u32 {
        KIND_VERSION
    }

    fn params(&self) -> ParamSchema {
        asset_schema()
    }

    fn traits(&self) -> Traits {
        Traits {
            time_varying: false,
            ..Traits::PIXEL
        }
    }

    fn migrate(&self, old: serde_json::Value, from: u32) -> Result<Params> {
        migrate_same_version(self, old, from)
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        render_source_frame(cx, RationalTime::ZERO)
    }
}
