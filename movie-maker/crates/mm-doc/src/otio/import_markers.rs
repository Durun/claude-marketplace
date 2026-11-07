//! OTIO のマーカーと、マーカーを置くコンポジションの組み立て。

use serde_json::Value;

use mm_types::{Id, MmError, ParamValue, Rational, RationalTime, Result};

use super::import::{Ctx, COMPOSITION, LAYER, MARKER};
use super::import_track::State;
use super::time::range_from_json;
use super::{sequence_layer, TrackMeta, NAMESPACE};
use crate::{Composition, Layer, Marker, Node, SEQUENCE_KIND, SEQUENCE_PARAM};

/// マーカーを持つコンポジションの大きさ。OTIO は画面の大きさを持たないので既定にする。
const DEFAULT_WIDTH: u32 = 1920;
const DEFAULT_HEIGHT: u32 = 1080;
/// fps が読めないときの値。
const FALLBACK_FPS: u32 = 30;
/// 同じ時刻のマーカーを後ろへずらす幅（秒の分母）。マーカーは同じ時刻に 2 つ置けない。
const MARKER_NUDGE_DEN: u32 = 1000;
/// 映像の素材のノードの種類のバージョン。
const NODE_VERSION: u32 = 1;

/// トラックの時間軸のマーカー。
pub(super) struct RawMarker {
    name: String,
    time: RationalTime,
    duration: RationalTime,
    id: Option<String>,
}

/// トラック自身（と Stack）のマーカー。時刻はトラックの時間軸。
pub(super) fn track_markers(track: &Value) -> Result<Vec<RawMarker>> {
    markers_in(track, Ok)
}

/// クリップのマーカー。時刻はクリップの素材の時間軸なので、トラックの時間軸に直して足す。
pub(super) fn collect_item_markers(
    item: &Value,
    st: &mut State,
    in_point: RationalTime,
    speed: Rational,
    enabled: bool,
) -> Result<()> {
    let head = st.head;
    let found = markers_in(item, |start| {
        if !enabled {
            return Ok(head);
        }
        head.checked_add(start.checked_sub(in_point)?.div_by(speed)?)
    })?;
    st.markers.extend(found);
    Ok(())
}

fn markers_in(
    item: &Value,
    to_track: impl Fn(RationalTime) -> Result<RationalTime>,
) -> Result<Vec<RawMarker>> {
    let mut out = Vec::new();
    for m in item
        .get("markers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some((start, duration)) = range_from_json(m.get("marked_range"))? else {
            continue;
        };
        out.push(RawMarker {
            name: m
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            time: to_track(start)?,
            duration: duration.max(RationalTime::ZERO),
            id: m
                .pointer(&format!("/metadata/{NAMESPACE}/id"))
                .and_then(Value::as_str)
                .map(str::to_owned),
        });
    }
    Ok(out)
}

/// マーカーを置くコンポジション。メタデータのコンポジションがあればそれ、マーカーだけあれば既定の大きさで作る。
pub(super) fn build_composition(
    ctx: &mut Ctx,
    seq: &str,
    meta: Option<&TrackMeta>,
    st: &mut State,
) -> Result<Option<Composition>> {
    let from_meta = meta
        .and_then(|m| m.composition.clone())
        .filter(|c| sequence_layer(c, seq).is_some());
    let mut comp = match from_meta {
        Some(c) => {
            let taken = std::iter::once((COMPOSITION, c.id.as_str()))
                .chain(c.layers.iter().map(|l| (LAYER, l.id.as_str())))
                .find(|(kind, id)| ctx.is_used(kind, id));
            if let Some((kind, id)) = taken {
                return Err(MmError::InvalidArgument(format!(
                    "{kind} {id} は既に在る（--as で別の ID にして取り込める）"
                )));
            }
            c
        }
        None if st.markers.is_empty() => return Ok(None),
        None => match foreign_composition(ctx, seq, st)? {
            Some(c) => c,
            None => return Ok(None),
        },
    };
    comp.markers = place_markers(ctx, seq, &comp, std::mem::take(&mut st.markers))?;
    Ok(Some(comp))
}

fn foreign_composition(ctx: &mut Ctx, seq: &str, st: &State) -> Result<Option<Composition>> {
    if st.head <= RationalTime::ZERO {
        ctx.warn("長さ 0 のトラックのマーカーは取り込まない".into());
        return Ok(None);
    }
    let fps = st.rate.unwrap_or(Rational::integer(FALLBACK_FPS));
    let comp_id = ctx.take_id(COMPOSITION, Some(seq), &format!("{seq}-comp"));
    let layer_id = ctx.take_id(LAYER, None, &format!("{seq}-layer"));
    let source = Node::new(SEQUENCE_KIND, NODE_VERSION)
        .with_param(SEQUENCE_PARAM, ParamValue::Text(seq.to_owned()));
    let layer = Layer::new(
        layer_id,
        source,
        RationalTime::ZERO,
        RationalTime::ZERO,
        st.head,
    );
    Ok(Some(
        Composition::new(
            Id::new(comp_id),
            DEFAULT_WIDTH,
            DEFAULT_HEIGHT,
            fps,
            st.head,
        )
        .with_layers(vec![layer]),
    ))
}

/// トラックの時間軸のマーカーを、コンポジションの時刻の厳密な昇順に並べる。同じ時刻は後ろへずらす。
fn place_markers(
    ctx: &mut Ctx,
    seq: &str,
    comp: &Composition,
    raw: Vec<RawMarker>,
) -> Result<Vec<Marker>> {
    let (start, in_point, speed) = match sequence_layer(comp, seq) {
        Some(l) => (l.start, l.in_point, l.speed),
        None => (RationalTime::ZERO, RationalTime::ZERO, Rational::integer(1)),
    };
    let mut timed = Vec::with_capacity(raw.len());
    for m in raw {
        let time = m
            .time
            .checked_sub(in_point)?
            .div_by(speed)?
            .checked_add(start)?;
        timed.push((time, m));
    }
    timed.sort_by_key(|(t, _)| *t);
    let nudge = RationalTime::new(1, MARKER_NUDGE_DEN)?;
    let mut out: Vec<Marker> = Vec::with_capacity(timed.len());
    for (mut time, m) in timed {
        if let Some(prev) = out.last().filter(|p| p.time >= time) {
            time = prev.time.checked_add(nudge)?;
            ctx.warn_once("同じ時刻のマーカーは 1/1000 秒ずつ後ろへずらした".into());
        }
        let id = ctx.take_id(
            MARKER,
            m.id.as_deref().filter(|_| ctx.keep_ids),
            &format!("{seq}-m"),
        );
        out.push(Marker {
            id: Id::new(id),
            time,
            duration: m.duration.div_by(speed)?,
            name: m.name,
        });
    }
    Ok(out)
}
