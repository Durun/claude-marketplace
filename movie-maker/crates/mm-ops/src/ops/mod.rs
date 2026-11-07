//! 操作。1 つ 1 ファイル。各ファイルは引数の型と [`crate::op::Apply`] の実装を持つ。

use mm_doc::{
    Composition, Cut, CutId, Layer, Node, Project, Sequence, NESTED_KIND, NESTED_PARAM,
    SEQUENCE_KIND, SEQUENCE_PARAM,
};
use mm_types::{CompositionId, LayerId, MmError, ParamValue, RationalTime, Result};

use crate::touches::Touches;

mod add_asset;
mod add_composition;
mod add_layer;
mod add_marker;
mod add_mask;
mod add_render_target;
mod add_sequence;
mod attach_layer;
mod disable;
mod extract;
mod fork_sequence;
mod insert;
mod join;
mod lock_layer;
mod move_keyframe;
mod move_layer;
mod override_cut;
mod overwrite;
mod parent_layer;
mod precompose;
mod remove_asset;
mod remove_composition;
mod remove_keyframe;
mod remove_layer;
mod remove_marker;
mod remove_mask;
mod remove_param;
mod remove_render_target;
mod remove_sequence;
mod reset_cut;
mod ripple;
mod roll;
mod set_caption;
mod set_ease;
mod set_keyframe;
mod set_layer_audio;
mod set_mixer;
mod set_param;
mod set_reframe;
mod set_transition;
mod slide;
mod slip;
mod split;
mod unprecompose;

mod add_selection;
mod add_topic;
mod bins;
mod cut_words;
mod delete_words;
mod extract_cuts;
mod fit_layer;
mod fix_transcript;
mod insert_cuts;
mod insert_topics;
mod lift;
mod mark_in;
mod mark_out;
mod rate;
mod remove_selection;
mod remove_topic;
mod tag;
mod trim_layer;

pub use add_selection::AddSelection;
pub use add_topic::AddTopic;
pub use cut_words::{CutSplice, CutWords, SourceRange};
pub use delete_words::DeleteWords;
pub use extract_cuts::ExtractCuts;
pub use fit_layer::{with_fitted_layers, FitLayer};
pub use fix_transcript::FixTranscript;
pub use insert_cuts::InsertCuts;
pub use insert_topics::InsertTopics;
pub use lift::Lift;
pub use mark_in::MarkIn;
pub use mark_out::MarkOut;
pub use rate::Rate;
pub use remove_selection::RemoveSelection;
pub use remove_topic::RemoveTopic;
pub use tag::Tag;
pub use trim_layer::TrimLayer;

pub use add_asset::AddAsset;
pub use add_composition::AddComposition;
pub use add_layer::AddLayer;
pub use add_marker::AddMarker;
pub use add_mask::AddMask;
pub use add_render_target::AddRenderTarget;
pub use add_sequence::AddSequence;
pub use attach_layer::AttachLayer;
pub use disable::Disable;
pub use extract::Extract;
pub use fork_sequence::{CutRange, ForkSequence};
pub use insert::Insert;
pub use join::Join;
pub use lock_layer::LockLayer;
pub use move_keyframe::MoveKeyframe;
pub use move_layer::MoveLayer;
pub use override_cut::OverrideCut;
pub use overwrite::Overwrite;
pub use parent_layer::ParentLayer;
pub use precompose::Precompose;
pub use remove_asset::RemoveAsset;
pub use remove_composition::RemoveComposition;
pub use remove_keyframe::RemoveKeyframe;
pub use remove_layer::RemoveLayer;
pub use remove_marker::RemoveMarker;
pub use remove_mask::RemoveMask;
pub use remove_param::RemoveParam;
pub use remove_render_target::RemoveRenderTarget;
pub use remove_sequence::RemoveSequence;
pub use reset_cut::ResetCut;
pub use ripple::{Edge, Ripple};
pub use roll::Roll;
pub use set_caption::SetCaption;
pub use set_ease::SetEase;
pub use set_keyframe::SetKeyframe;
pub use set_layer_audio::SetLayerAudio;
pub use set_mixer::SetMixer;
pub use set_param::SetParam;
pub use set_reframe::SetReframe;
pub use set_transition::SetTransition;
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

// `touches` は書く所だけでなく読む所（参照先、参照元、シーケンスの隣）も含める。
// 重ならない 2 つの操作は順を入れ替えても同じプロジェクトのデータになる、という性質を保つため。

/// ノードが参照するコンポジション、sequence、素材。素材を消す操作（`removeAsset`）と順が入れ替わらないよう、
/// 撮影素材や LUT の素材の参照も読む所に入れる。
pub(crate) fn node_refs(node: &Node, t: &mut Touches) {
    for n in node.descendants() {
        for param in n.params.values() {
            if let ParamValue::AssetRef(asset) = &param.value {
                t.add(asset);
            }
        }
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

/// カットが参照する所（入れ子、トランジションの入れ子、参照・複製の元のカット）。
pub(crate) fn cut_refs(cut: &Cut, t: &mut Touches) {
    node_refs(&cut.source, t);
    if let Some(origin) = &cut.origin {
        t.add(origin);
    }
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

/// sequence を参照する所（ノードの種類が `sequence` のレイヤーとカット、カットに接続したレイヤー、分岐した sequence）。
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

/// 入れ子の先（コンポジションと sequence）のうち、`id` の内容が直接指す ID。
fn direct_nesting(project: &Project, id: &str) -> Touches {
    let mut refs = Touches::new();
    if let Some(comp) = project.compositions.iter().find(|c| c.id.as_str() == id) {
        comp.layers
            .iter()
            .flat_map(Layer::nodes)
            .for_each(|n| node_refs(n, &mut refs));
    }
    if let Some(seq) = project.sequences.iter().find(|s| s.id.as_str() == id) {
        seq.cuts
            .iter()
            .flat_map(Cut::nodes)
            .for_each(|n| node_refs(n, &mut refs));
    }
    refs
}

/// `roots` の ID から入れ子でたどれる先の全部を `t` に足す（`roots` も含む）。
///
/// 入れ子の循環を作る操作は、参照先の先を読んでいる。先の内容を書き換える操作の `touches` と
/// 重なるよう、読む側が先の全部を持つ。
pub(crate) fn add_nesting_reach(project: &Project, roots: &Touches, t: &mut Touches) {
    let mut seen = std::collections::HashSet::new();
    let mut queue: Vec<String> = roots.iter().map(str::to_owned).collect();
    while let Some(id) = queue.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        t.add_str(&id);
        queue.extend(direct_nesting(project, &id).iter().map(str::to_owned));
    }
}

/// レイヤーが参照する所と、その入れ子の先の全部。
pub(crate) fn layer_refs_reach(project: &Project, layer: &Layer, t: &mut Touches) {
    layer_refs(layer, t);
    let mut roots = Touches::new();
    layer.nodes().for_each(|n| node_refs(n, &mut roots));
    add_nesting_reach(project, &roots, t);
}

/// カットが参照する所と、その入れ子の先の全部。
pub(crate) fn cut_refs_reach(project: &Project, cut: &Cut, t: &mut Touches) {
    cut_refs(cut, t);
    let mut roots = Touches::new();
    cut.nodes().for_each(|n| node_refs(n, &mut roots));
    add_nesting_reach(project, &roots, t);
}

/// `id` から親をたどった祖先のレイヤー（`id` 自身を除く）。
pub(crate) fn add_ancestors(comp: &Composition, id: &LayerId, t: &mut Touches) {
    let mut seen = std::collections::HashSet::new();
    let mut cur = comp.layer(id).ok().and_then(|l| l.parent.as_ref());
    while let Some(p) = cur {
        if !seen.insert(p.as_str()) {
            break;
        }
        t.add(p);
        cur = comp.layer(p).ok().and_then(|l| l.parent.as_ref());
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
