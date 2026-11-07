//! OTIO の映像トラック 1 本から sequence への変換。

use serde_json::Value;

use mm_types::{AssetId, Id, MmError, Rational, RationalTime, Result};

use super::export::{DISSOLVE_KIND, DISSOLVE_TYPE};
use super::import::{slug, Ctx, ImportOptions, CUT, SEQUENCE};
use super::import_markers::{build_composition, collect_item_markers, track_markers, RawMarker};
use super::time::{near, near_speed, range_from_json, rational_from_f64, time_from_json};
use super::{schema_name, CutMeta, TrackMeta, META_VERSION};
use crate::{Composition, Cut, CutOverrides, Node, Sequence, Transition};

/// 映像の素材のノードの種類のバージョン。
const NODE_VERSION: u32 = 1;
/// OTIO の `Clip.2` の既定の素材の参照のキー。
const DEFAULT_MEDIA_KEY: &str = "DEFAULT_MEDIA";

/// トランジションの項目から読んだ、次のカットに掛ける値。
struct PendingTransition {
    duration: RationalTime,
    kind: String,
}

/// トラックを読む間の状態。
pub(super) struct State {
    pub cuts: Vec<Cut>,
    /// 有効なカットを置いた終わりの時刻。
    pub head: RationalTime,
    pub markers: Vec<RawMarker>,
    pending: Option<PendingTransition>,
    /// 最初に見つけた時刻の rate。マーカーのコンポジションの fps に使う。
    pub rate: Option<Rational>,
}

fn invalid(msg: String) -> MmError {
    MmError::InvalidArgument(msg)
}

/// トラックのメタデータ。ID を作り直す取り込み（`--as`）では使わず、捨てるコンポジションの設定を警告に出す。
/// 形のバージョンが違えば、読めた分を使ったうえで警告する。
fn track_meta(ctx: &mut Ctx, track: &Value) -> Option<TrackMeta> {
    let meta: TrackMeta = ctx.meta(track)?;
    if meta.version != META_VERSION {
        ctx.warn(format!(
            "トラックのメタデータの形のバージョン {} はこのビルドの {META_VERSION} と違う",
            meta.version
        ));
    }
    if ctx.keep_ids {
        return Some(meta);
    }
    if let Some(c) = &meta.composition {
        ctx.warn(format!(
            "ID を作り直す取り込みでは、コンポジション {} の設定（大きさ、fps、ミキサー、レイヤーの音と変形）を取り込まない",
            c.id
        ));
    }
    None
}

/// 映像のトラック 1 本を sequence にする。マーカーかコンポジションのメタデータがあればコンポジションも返す。
pub(super) fn import_track(
    ctx: &mut Ctx,
    track: &Value,
    base: &str,
    index: usize,
    count: usize,
    opts: &ImportOptions,
) -> Result<(Sequence, Option<Composition>)> {
    let meta = track_meta(ctx, track);
    let seq_id = sequence_id(ctx, track, meta.as_ref(), base, index, count, opts)?;
    let mut st = State {
        cuts: Vec::new(),
        head: RationalTime::ZERO,
        markers: track_markers(track)?,
        pending: None,
        rate: None,
    };
    let children = track.get("children").and_then(Value::as_array);
    for child in children.into_iter().flatten() {
        import_child(ctx, &seq_id, child, &mut st)?;
    }
    if st.pending.take().is_some() {
        ctx.warn("末尾のトランジションは次のクリップが無いので取り込まない".into());
    }
    let comp = build_composition(ctx, &seq_id, meta.as_ref(), &mut st)?;
    let seq = Sequence {
        id: Id::new(seq_id),
        cuts: st.cuts,
        origin: meta.and_then(|m| m.origin),
    };
    Ok((seq, comp))
}

fn sequence_id(
    ctx: &mut Ctx,
    track: &Value,
    meta: Option<&TrackMeta>,
    base: &str,
    index: usize,
    count: usize,
    opts: &ImportOptions,
) -> Result<String> {
    let track_name = track.get("name").and_then(Value::as_str).unwrap_or("");
    let name = if count == 1 {
        base.to_owned()
    } else if opts.sequence_id.is_some() || track_name.is_empty() {
        format!("{base}-{}", index + 1)
    } else {
        format!("{base}-{}", slug(track_name))
    };
    let preferred = meta.map(|m| m.sequence.clone());
    let explicit = preferred
        .as_deref()
        .or(opts.sequence_id.as_deref().map(|_| name.as_str()));
    if let Some(id) = explicit.filter(|id| ctx.is_used(SEQUENCE, id)) {
        return Err(invalid(format!(
            "sequence {id} は既に在る（--as で別の ID にして取り込める）"
        )));
    }
    let want = preferred.unwrap_or(name.clone());
    Ok(ctx.take_id(SEQUENCE, Some(&want), &format!("{name}-")))
}

fn import_child(ctx: &mut Ctx, seq: &str, child: &Value, st: &mut State) -> Result<()> {
    match schema_name(child) {
        Some("Clip" | "Gap") => {
            if let Some((cut, meta_transition)) = import_item(ctx, seq, child, st)? {
                place_cut(ctx, cut, meta_transition, st)?;
            }
        }
        Some("Transition") => read_transition(ctx, child, st)?,
        Some("Track" | "Sequence" | "Stack") => nested_as_gap(ctx, seq, child, st)?,
        other => ctx.warn(format!("トラックの子 {other:?} は取り込まない")),
    }
    Ok(())
}

/// 入れ子の Track・Stack は中身を取り込めないので、長さを保つ隙間にする。
fn nested_as_gap(ctx: &mut Ctx, seq: &str, nested: &Value, st: &mut State) -> Result<()> {
    let name = nested.get("name").and_then(Value::as_str).unwrap_or("");
    let Some(duration) = item_duration(nested)? else {
        ctx.warn(format!(
            "入れ子 {name:?} の長さが分からないので取り込まない"
        ));
        return Ok(());
    };
    ctx.warn(format!("入れ子 {name:?} は取り込めないので隙間にした"));
    if duration > RationalTime::ZERO {
        let id = ctx.take_id(CUT, None, &format!("{seq}-c"));
        let cut = Cut::new(id, Node::gap(), RationalTime::ZERO, duration);
        place_cut(ctx, cut, None, st)?;
    }
    Ok(())
}

/// 項目の長さ。`source_range` があればその長さ、Track は子の長さの和、Stack は子の長さの最大。
fn item_duration(item: &Value) -> Result<Option<RationalTime>> {
    if let Some((_, d)) = range_from_json(item.get("source_range"))? {
        return Ok(Some(d));
    }
    let children = item.get("children").and_then(Value::as_array);
    let mut lens = Vec::new();
    for child in children.into_iter().flatten() {
        if schema_name(child) == Some("Transition") {
            continue;
        }
        match item_duration(child)? {
            Some(d) => lens.push(d),
            None => return Ok(None),
        }
    }
    if schema_name(item) == Some("Stack") {
        return Ok(Some(lens.into_iter().max().unwrap_or(RationalTime::ZERO)));
    }
    Ok(Some(
        lens.into_iter()
            .try_fold(RationalTime::ZERO, |a, d| a.checked_add(d))?,
    ))
}

fn read_transition(ctx: &mut Ctx, item: &Value, st: &mut State) -> Result<()> {
    let offset = |name: &str| {
        item.get(name)
            .map_or(Ok(RationalTime::ZERO), time_from_json)
    };
    let duration = offset("in_offset")?.checked_add(offset("out_offset")?)?;
    if st.cuts.is_empty() || duration <= RationalTime::ZERO {
        ctx.warn("先頭か長さ 0 のトランジションは取り込まない".into());
        return Ok(());
    }
    let kind = item
        .get("transition_type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    st.pending = Some(PendingTransition { duration, kind });
    Ok(())
}

/// カットを並べる。待っているトランジションがあれば `transition_in` にする。
fn place_cut(
    ctx: &mut Ctx,
    mut cut: Cut,
    meta_transition: Option<Transition>,
    st: &mut State,
) -> Result<()> {
    cut.transition_in = match (st.pending.take(), meta_transition) {
        (Some(p), Some(m)) if near(m.duration, p.duration) => Some(m),
        (Some(p), _) => Some(transition_from_type(ctx, p)),
        (None, Some(m)) if st.cuts.is_empty() => Some(m),
        (None, _) => None,
    };
    if cut.enabled {
        st.head = st.head.checked_add(cut.duration()?)?;
    }
    st.cuts.push(cut);
    Ok(())
}

fn transition_from_type(ctx: &mut Ctx, p: PendingTransition) -> Transition {
    if p.kind != DISSOLVE_TYPE {
        ctx.warn_once(format!(
            "トランジションの種類 {:?} は取り込めないのでディゾルブにした",
            p.kind
        ));
    }
    Transition {
        node: Node::new(DISSOLVE_KIND, NODE_VERSION),
        duration: p.duration,
    }
}

/// `Clip` か `Gap` 1 つをカットにする。取り込めなければ `None`。
fn import_item(
    ctx: &mut Ctx,
    seq: &str,
    item: &Value,
    st: &mut State,
) -> Result<Option<(Cut, Option<Transition>)>> {
    let is_gap = schema_name(item) == Some("Gap");
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let reference = if is_gap { None } else { media_reference(item) };
    let Some((start, duration)) = item_range(item, reference)? else {
        ctx.warn(format!("{name:?} は長さが分からないので取り込まない"));
        return Ok(None);
    };
    note_rate(item, st);
    let meta: Option<CutMeta> = ctx.meta(item);
    let (enabled, gap) = item_state(ctx, item, &name, is_gap, duration, meta.as_ref());
    if enabled && duration <= RationalTime::ZERO {
        ctx.warn(format!("{name:?} は長さが 0 なので取り込まない"));
        return Ok(None);
    }
    let scalar = read_effects(ctx, item, &name);
    let (in_point, out_point, speed) = match &meta {
        Some(m) if !enabled => (m.in_point, m.out_point, m.speed),
        _ => reconcile(meta.as_ref(), gap, start, duration, scalar)?,
    };
    let (node, asset) = build_node(ctx, reference, meta.as_ref(), gap, &name)?;
    if let Some(asset) = asset {
        ctx.note_extent(&asset, out_point);
    }
    collect_item_markers(item, st, in_point, speed, enabled)?;
    let mut cut = Cut {
        id: Id::new(""),
        source: node,
        in_point,
        out_point,
        speed,
        enabled,
        transition_in: None,
        origin: None,
        overrides: CutOverrides::default(),
        origin_base: None,
    };
    assign_identity(ctx, seq, &mut cut, meta.as_ref());
    Ok(Some((cut, meta.and_then(|m| m.transition_in))))
}

/// 項目の `source_range`。無ければ素材の `available_range`。
fn item_range(
    item: &Value,
    reference: Option<&Value>,
) -> Result<Option<(RationalTime, RationalTime)>> {
    if let Some(r) = range_from_json(item.get("source_range"))? {
        return Ok(Some(r));
    }
    match reference {
        Some(r) => range_from_json(r.get("available_range")),
        None => Ok(None),
    }
}

/// カットの ID と元との関係を決める。メタデータの値は、ID を引き継ぐ取り込みのときだけ使う。
fn assign_identity(ctx: &mut Ctx, seq: &str, cut: &mut Cut, meta: Option<&CutMeta>) {
    let kept = meta.filter(|_| ctx.keep_ids);
    let id = ctx.take_id(CUT, kept.map(|m| m.id.as_str()), &format!("{seq}-c"));
    cut.id = Id::new(id);
    if let Some(m) = kept {
        cut.origin = m.origin.clone();
        cut.overrides = m.overrides;
        cut.origin_base = m.origin_base.clone();
    }
}

/// 項目が有効か、隙間として扱うか。
///
/// OTIO は無効な項目も長さを数える。movie-maker の外したカットは長さ 0 で書くので、長さ 0 でメタデータの
/// `enabled` が偽なら外したカット。ほかの無効な項目は、長さを保つために隙間にする。
fn item_state(
    ctx: &mut Ctx,
    item: &Value,
    name: &str,
    is_gap: bool,
    duration: RationalTime,
    meta: Option<&CutMeta>,
) -> (bool, bool) {
    let native_enabled = item.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    if native_enabled {
        return (true, is_gap);
    }
    if duration == RationalTime::ZERO && meta.is_some_and(|m| !m.enabled) {
        return (false, is_gap);
    }
    ctx.warn(format!("無効な {name:?} は長さを保つ隙間にした"));
    (true, true)
}

/// OTIO の値と、メタデータの厳密な値から、カットの in、out、速さを決める。
///
/// OTIO の `source_range` の長さは配置の長さ。素材は `in` から `長さ × 速さ` だけ進む。
/// メタデータが OTIO の値と合っていればメタデータ（厳密な有理数）を使い、食い違えば OTIO の値に従う。
fn reconcile(
    meta: Option<&CutMeta>,
    gap: bool,
    start: RationalTime,
    duration: RationalTime,
    scalar: Option<Rational>,
) -> Result<(RationalTime, RationalTime, Rational)> {
    let native_speed = scalar.unwrap_or(Rational::integer(1));
    let exact = meta.filter(|m| {
        if gap {
            m.out_point
                .checked_sub(m.in_point)
                .and_then(|d| d.div_by(m.speed))
                .is_ok_and(|d| near(d, duration))
        } else {
            near_speed(m.speed, native_speed) && near(m.in_point, start)
        }
    });
    let (in_point, speed) = match (exact, gap) {
        (Some(m), _) => (m.in_point, m.speed),
        (None, true) => (RationalTime::ZERO, Rational::integer(1)),
        (None, false) => (start, native_speed),
    };
    let out_point = in_point.checked_add(duration.scale(speed)?)?;
    Ok((in_point, out_point, speed))
}

/// 効果の列から速さを読む。`LinearTimeWarp` の `time_scalar` だけ取り込み、ほかは警告を出して捨てる。
fn read_effects(ctx: &mut Ctx, item: &Value, name: &str) -> Option<Rational> {
    let mut speed = None;
    for effect in item
        .get("effects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match schema_name(effect) {
            Some("LinearTimeWarp") => {
                let scalar = effect.get("time_scalar").and_then(Value::as_f64);
                match scalar.and_then(rational_from_f64) {
                    Some(s) => speed = Some(s),
                    None => ctx.warn(format!(
                        "{name:?} の速さ {scalar:?} は取り込めないので等速にした"
                    )),
                }
            }
            other => ctx.warn_once(format!("効果 {other:?} は取り込まない")),
        }
    }
    speed
}

/// クリップの素材の参照。`Clip.1` は `media_reference`、`Clip.2` は `media_references` の使用中のキー。
fn media_reference(item: &Value) -> Option<&Value> {
    if let Some(r) = item.get("media_reference").filter(|r| !r.is_null()) {
        return Some(r);
    }
    let key = item
        .get("active_media_reference_key")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_MEDIA_KEY);
    item.get("media_references")?.get(key)
}

/// カットの素材のノード。撮影素材は素材を割り当て、素材の無いものはメタデータのノードか、無ければ隙間。
fn build_node(
    ctx: &mut Ctx,
    reference: Option<&Value>,
    meta: Option<&CutMeta>,
    gap: bool,
    name: &str,
) -> Result<(Node, Option<AssetId>)> {
    if gap {
        return Ok((Node::gap(), None));
    }
    let meta_node = meta.and_then(|m| m.node.clone());
    let external = reference.filter(|r| schema_name(r) == Some("ExternalReference"));
    if let Some(r) = external {
        let id = ctx.resolve_asset(r)?;
        let node = match meta_node {
            Some(mut n) => {
                ctx.remap_node(&mut n);
                n
            }
            None => Node::footage(&id),
        };
        return Ok((node, Some(id)));
    }
    match meta_node {
        Some(mut n) => {
            ctx.remap_node(&mut n);
            Ok((n, None))
        }
        None => {
            let kind = reference.and_then(schema_name).unwrap_or("参照なし");
            ctx.warn(format!(
                "クリップ {name:?} の素材（{kind}）は取り込めないので隙間にした"
            ));
            Ok((Node::gap(), None))
        }
    }
}

fn note_rate(item: &Value, st: &mut State) {
    if st.rate.is_some() {
        return;
    }
    st.rate = item
        .pointer("/source_range/start_time/rate")
        .and_then(Value::as_f64)
        .and_then(rational_from_f64);
}
