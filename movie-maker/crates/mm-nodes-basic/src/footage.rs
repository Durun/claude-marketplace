use mm_audio_api::AudioBlock;
use mm_node_api::{audio_frames_in, AudioCx, FrameCx, FrameRef, NodeKind, Traits};
use mm_types::{
    Id, MmError, ParamSchema, ParamSpec, ParamType, ParamValue, Params, RationalTime, Rect, Result,
};

use crate::util::{fit_contain, migrate_same_version};
use crate::KIND_VERSION;

/// 素材の置き方のパラメータ。`contain`（既定）は縦横比を保って画布に収め、`none` は素材の画素のまま左上に置く。
pub const FIT_PARAM: &str = "fit";
/// `fit` の値。
pub const FIT_CONTAIN: &str = "contain";
/// `fit` の値。
pub const FIT_NONE: &str = "none";

fn asset_schema() -> ParamSchema {
    ParamSchema::new(vec![
        ParamSpec::new(
            "asset",
            ParamType::AssetRef,
            ParamValue::AssetRef(Id::new("")),
        ),
        ParamSpec::new(
            FIT_PARAM,
            ParamType::Choice {
                options: vec![FIT_CONTAIN.into(), FIT_NONE.into()],
            },
            ParamValue::Choice(FIT_CONTAIN.into()),
        ),
    ])
}

/// `fit` が `none` か。無ければ `contain`。
fn fit_none(cx: &FrameCx<'_>) -> bool {
    cx.params.get(FIT_PARAM).and_then(ParamValue::as_text) == Some(FIT_NONE)
}

/// 素材の `t` のフレーム。素材が無ければ透明。
/// GPU のフレームは素材のままの大きさで借用し（`GpuHelper::import`）、レンダラーが `region_of_definition` の
/// 矩形へ 1 回だけ縮小する。CPU のフレームは `contain` なら画布に収めて返し（画像の大きさは画布と同じという契約）、
/// `none` なら素材の大きさのまま返して、レンダラーが領域へ置く。
fn render_source_frame(cx: &mut FrameCx<'_>, t: RationalTime) -> Result<FrameRef> {
    let size = cx.size;
    let keep = fit_none(cx);
    let Some(footage) = cx.footage.as_deref_mut() else {
        return Ok(FrameRef::Transparent { size });
    };
    let frame = footage.frame_at(t)?;
    match (frame.image, frame.gpu) {
        (Some(image), _) if keep => Ok(FrameRef::cpu(image)),
        (Some(image), _) => Ok(FrameRef::cpu(fit_contain(image, size))),
        (None, Some(handle)) => cx.gpu.import(&handle),
        (None, None) => Err(MmError::Corrupt(
            "デコーダーが画像も GPU のフレームも返さない".into(),
        )),
    }
}

/// 素材を置く矩形（画布の画素）。素材が無ければ画布の全体、映像の無い素材（音だけ）なら空。
fn picture_region(cx: &FrameCx<'_>) -> Rect {
    let canvas = cx.size.to_rect();
    let Some(media) = cx.footage.as_deref().map(|f| f.info().size) else {
        return canvas;
    };
    if media.width == 0 || media.height == 0 {
        return Rect::new(0.0, 0.0, 0.0, 0.0);
    }
    if fit_none(cx) {
        return Rect::new(
            0.0,
            0.0,
            media.width as f32 * cx.scale,
            media.height as f32 * cx.scale,
        );
    }
    let ratio = (canvas.width / media.width as f32).min(canvas.height / media.height as f32);
    let (w, h) = (
        (media.width as f32 * ratio).round().max(1.0),
        (media.height as f32 * ratio).round().max(1.0),
    );
    Rect::new(
        ((canvas.width - w) / 2.0).floor(),
        ((canvas.height - h) / 2.0).floor(),
        w,
        h,
    )
}

/// 撮影素材。`asset` の素材をレンダラーが開いて `cx.footage` に渡し、`cx.source_time` のフレームを返す。
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

    fn region_of_definition(&self, cx: &FrameCx<'_>) -> Rect {
        picture_region(cx)
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

/// 静止画。素材の最初のフレームを使い、時間で変わらない。
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

    fn region_of_definition(&self, cx: &FrameCx<'_>) -> Rect {
        picture_region(cx)
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        render_source_frame(cx, RationalTime::ZERO)
    }
}
