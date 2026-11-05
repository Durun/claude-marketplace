//! 命令。1 つ 1 ファイル。各ファイルは引数の型と [`crate::op::Apply`] の実装を持つ。

use mm_doc::{
    Composition, Cut, CutId, Layer, Node, Project, Sequence, NESTED_KIND, NESTED_PARAM,
    SEQUENCE_KIND, SEQUENCE_PARAM,
};
use mm_types::{CompositionId, LayerId, MmError, RationalTime, Result};

use crate::touches::Touches;

mod add_composition;
mod add_layer;
mod add_marker;
mod add_render_target;
mod add_sequence;
mod attach_layer;
mod disable;
mod extract;
mod fork_sequence;
mod insert;
mod join;
mod lock_layer;
mod move_layer;
mod overwrite;
mod parent_layer;
mod precompose;
mod remove_composition;
mod remove_keyframe;
mod remove_layer;
mod remove_marker;
mod remove_param;
mod remove_render_target;
mod remove_sequence;
mod ripple;
mod roll;
mod set_ease;
mod set_keyframe;
mod set_param;
mod slide;
mod slip;
mod split;
mod unprecompose;

pub use add_composition::AddComposition;
pub use add_layer::AddLayer;
pub use add_marker::AddMarker;
pub use add_render_target::AddRenderTarget;
pub use add_sequence::AddSequence;
pub use attach_layer::AttachLayer;
pub use disable::Disable;
pub use extract::Extract;
pub use fork_sequence::ForkSequence;
pub use insert::Insert;
pub use join::Join;
pub use lock_layer::LockLayer;
pub use move_layer::MoveLayer;
pub use overwrite::Overwrite;
pub use parent_layer::ParentLayer;
pub use precompose::Precompose;
pub use remove_composition::RemoveComposition;
pub use remove_keyframe::RemoveKeyframe;
pub use remove_layer::RemoveLayer;
pub use remove_marker::RemoveMarker;
pub use remove_param::RemoveParam;
pub use remove_render_target::RemoveRenderTarget;
pub use remove_sequence::RemoveSequence;
pub use ripple::{Edge, Ripple};
pub use roll::Roll;
pub use set_ease::SetEase;
pub use set_keyframe::SetKeyframe;
pub use set_param::SetParam;
pub use slide::Slide;
pub use slip::Slip;
pub use split::Split;
pub use unprecompose::Unprecompose;

/// 引数の誤り。
pub(crate) fn invalid(msg: impl Into<String>) -> MmError {
    MmError::InvalidArgument(msg.into())
}

/// `-t`。
pub(crate) fn negate(t: RationalTime) -> Result<RationalTime> {
    RationalTime::ZERO.checked_sub(t)
}

// `touches` は書く所だけでなく読む所（参照先、参照元、並びの隣）も含める。
// 重ならない 2 つの命令は順を入れ替えても同じ文書になる、という性質を保つため。

/// 動画オブジェクトが参照するコンポジションと sequence。
pub(crate) fn node_refs(node: &Node, t: &mut Touches) {
    for n in node.descendants() {
        if n.kind == NESTED_KIND {
            t.add_str(n.text_param(NESTED_PARAM).unwrap_or(""));
        }
        if n.kind == SEQUENCE_KIND {
            t.add_str(n.text_param(SEQUENCE_PARAM).unwrap_or(""));
        }
    }
}

/// レイヤーが参照する所（親、接続先、トラックマット、入れ子、効果の入れ子）。
pub(crate) fn layer_refs(layer: &Layer, t: &mut Touches) {
    if let Some(p) = &layer.parent {
        t.add(p);
    }
    if let Some(c) = &layer.attach_to {
        t.add(c);
    }
    if let Some(m) = &layer.track_matte {
        t.add(&m.layer);
    }
    node_refs(&layer.source, t);
    for e in &layer.effects {
        node_refs(&e.node, t);
    }
}

/// カットが参照する所（入れ子、トランジションの入れ子）。
pub(crate) fn cut_refs(cut: &Cut, t: &mut Touches) {
    node_refs(&cut.source, t);
    if let Some(tr) = &cut.transition_in {
        node_refs(&tr.node, t);
    }
}

/// `index` の位置に差し込むときの前後のカット。
pub(crate) fn cuts_around_gap(seq: &Sequence, index: usize, t: &mut Touches) {
    if let Some(prev) = index.checked_sub(1).and_then(|i| seq.cuts.get(i)) {
        t.add(&prev.id);
    }
    if let Some(next) = seq.cuts.get(index) {
        t.add(&next.id);
    }
}

/// `index` のカットの前後のカット。
pub(crate) fn cuts_beside(seq: &Sequence, index: usize, t: &mut Touches) {
    cuts_around_gap(seq, index, t);
    if let Some(next) = seq.cuts.get(index + 1) {
        t.add(&next.id);
    }
}

/// コンポジションを参照する所（レンダーターゲット、入れ子にするレイヤーとカット）。
pub(crate) fn composition_referrers(project: &Project, id: &CompositionId, t: &mut Touches) {
    for target in project
        .render_targets
        .iter()
        .filter(|r| &r.composition == id)
    {
        t.add(&target.id);
    }
    let layers = project.compositions.iter().flat_map(|c| c.layers.iter());
    for layer in layers {
        let mut refs = Touches::new();
        layer_refs(layer, &mut refs);
        if refs.iter().any(|r| r == id.as_str()) {
            t.add(&layer.id);
        }
    }
    for cut in project.sequences.iter().flat_map(|s| s.cuts.iter()) {
        let mut refs = Touches::new();
        cut_refs(cut, &mut refs);
        if refs.iter().any(|r| r == id.as_str()) {
            t.add(&cut.id);
        }
    }
}

/// sequence を参照する所（種類 `sequence` のレイヤーとカット、カットに接続したレイヤー、分岐した sequence）。
pub(crate) fn sequence_referrers(project: &Project, seq: &Sequence, t: &mut Touches) {
    let cut_ids: Vec<&str> = seq.cuts.iter().map(|c| c.id.as_str()).collect();
    let layers = project.compositions.iter().flat_map(|c| c.layers.iter());
    for layer in layers {
        let mut refs = Touches::new();
        layer_refs(layer, &mut refs);
        let hits = refs
            .iter()
            .any(|r| r == seq.id.as_str() || cut_ids.contains(&r));
        if hits {
            t.add(&layer.id);
        }
    }
    for cut in project.sequences.iter().flat_map(|s| s.cuts.iter()) {
        let mut refs = Touches::new();
        cut_refs(cut, &mut refs);
        if refs.iter().any(|r| r == seq.id.as_str()) {
            t.add(&cut.id);
        }
    }
    for other in &project.sequences {
        if other.origin.as_ref().is_some_and(|o| o.sequence == seq.id) {
            t.add(&other.id);
        }
    }
}

/// カットに接続したレイヤー。
pub(crate) fn cut_referrers(project: &Project, id: &CutId, t: &mut Touches) {
    let layers = project.compositions.iter().flat_map(|c| c.layers.iter());
    for layer in layers.filter(|l| l.attach_to.as_ref() == Some(id)) {
        t.add(&layer.id);
    }
}

/// レイヤーを親かトラックマットにするレイヤー。
pub(crate) fn layer_referrers(comp: &Composition, id: &LayerId, t: &mut Touches) {
    for layer in comp.layers.iter().filter(|l| {
        l.parent.as_ref() == Some(id) || l.track_matte.as_ref().is_some_and(|m| &m.layer == id)
    }) {
        t.add(&layer.id);
    }
}
