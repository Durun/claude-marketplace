//! 種から、参照を持つ動画オブジェクト・レイヤー・カットを組み立てる。

use crate::common::{cut, layer};
use mm_doc::{
    CutId, Effect, Layer, LayerAudio, Mask, MaskMode, MatteKind, Node, Param, Project,
    TimeKeyframe, TimeRemap, TrackMatte, Transition, NESTED_KIND, NESTED_PARAM, SEQUENCE_KIND,
    SEQUENCE_PARAM,
};
use mm_types::{Id, LayerId, ParamValue, RationalTime};
use proptest::prelude::*;

/// 動画オブジェクトの種。
#[derive(Debug, Clone)]
pub struct NodeSpec {
    /// 0 は solid、1 は入れ子のコンポジション、2 は sequence、3 は solid の入力に入れ子を持つ。
    pub kind: u8,
    pub pick: u8,
}

/// レイヤーの種。参照（親、接続、トラックマット、入れ子）と付属（効果、マスク、音、タイムリマップ）を持つ。
#[derive(Debug, Clone)]
pub struct LayerSpec {
    pub id: String,
    pub node: NodeSpec,
    pub parent: Option<u8>,
    pub attach: Option<u8>,
    pub matte: Option<u8>,
    pub effect: Option<NodeSpec>,
    pub mask: bool,
    pub audio: bool,
    pub remap: bool,
}

/// カットの種。
#[derive(Debug, Clone)]
pub struct CutSpec {
    pub id: String,
    pub node: NodeSpec,
    pub in_s: i64,
    pub len: i64,
    pub transition: Option<NodeSpec>,
    pub origin: Option<u8>,
}

/// ばらけた ID。小さな集まりから選ぶので、重なる場合も出る。
pub fn arb_id() -> impl Strategy<Value = String> {
    prop_oneof![
        1 => Just("lx".to_owned()),
        1 => Just("c.x".to_owned()),
        4 => "[a-z][a-z0-9_-]{0,3}",
    ]
}

pub fn arb_node() -> impl Strategy<Value = NodeSpec> {
    (0u8..4, any::<u8>()).prop_map(|(kind, pick)| NodeSpec { kind, pick })
}

pub fn arb_layer() -> impl Strategy<Value = LayerSpec> {
    let refs = (
        prop::option::weighted(0.3, any::<u8>()),
        prop::option::weighted(0.2, any::<u8>()),
        prop::option::weighted(0.2, any::<u8>()),
    );
    let extras = (
        prop::option::weighted(0.4, arb_node()),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    );
    (arb_id(), arb_node(), refs, extras).prop_map(
        |(id, node, (parent, attach, matte), (effect, mask, audio, remap))| LayerSpec {
            id,
            node,
            parent,
            attach,
            matte,
            effect,
            mask,
            audio,
            remap,
        },
    )
}

pub fn arb_cut() -> impl Strategy<Value = CutSpec> {
    (
        arb_id(),
        arb_node(),
        0i64..4,
        1i64..4,
        prop::option::weighted(0.3, arb_node()),
        prop::option::weighted(0.3, any::<u8>()),
    )
        .prop_map(|(id, node, in_s, len, transition, origin)| CutSpec {
            id,
            node,
            in_s,
            len,
            transition,
            origin,
        })
}

pub fn pick<T>(items: &[T], i: u8) -> Option<&T> {
    if items.is_empty() {
        None
    } else {
        items.get(usize::from(i) % items.len())
    }
}

pub fn layer_id(p: &Project, i: u8) -> Option<LayerId> {
    let layers: Vec<_> = p
        .compositions
        .iter()
        .flat_map(|c| c.layers.iter())
        .map(|l| l.id.clone())
        .collect();
    pick(&layers, i).cloned()
}

pub fn cut_id(p: &Project, i: u8) -> Option<CutId> {
    let cuts: Vec<_> = p
        .sequences
        .iter()
        .flat_map(|s| s.cuts.iter())
        .map(|c| c.id.clone())
        .collect();
    pick(&cuts, i).cloned()
}

fn text_node(kind: &str, param: &str, target: &str) -> Node {
    Node::new(kind, 1).with_param(param, ParamValue::Text(target.to_owned()))
}

/// 種を今の文書の ID を指す動画オブジェクトにする。指す先が無ければ solid。
pub fn build_node(spec: &NodeSpec, p: &Project) -> Node {
    let comp = pick(&p.compositions, spec.pick).map(|c| c.id.as_str());
    let seq = pick(&p.sequences, spec.pick).map(|s| s.id.as_str());
    match (spec.kind, comp, seq) {
        (1, Some(c), _) => text_node(NESTED_KIND, NESTED_PARAM, c),
        (2, _, Some(s)) => text_node(SEQUENCE_KIND, SEQUENCE_PARAM, s),
        (3, Some(c), _) => {
            let mut n = crate::common::solid();
            n.inputs.push(text_node(NESTED_KIND, NESTED_PARAM, c));
            n
        }
        _ => crate::common::solid(),
    }
}

fn mask_for(id: &str) -> Mask {
    let num = |v: f64| Param::constant(ParamValue::Number(v));
    Mask {
        id: Id::new(format!("{id}-mk")),
        path: num(0.0),
        mode: MaskMode::Add,
        inverted: false,
        feather: num(0.0),
        opacity: num(1.0),
    }
}

fn remap() -> TimeRemap {
    let key = |t: i64, interpolation| TimeKeyframe {
        time: RationalTime::seconds(t),
        source: RationalTime::seconds(t),
        interpolation,
    };
    TimeRemap {
        keyframes: vec![
            key(0, mm_doc::Interpolation::Linear),
            key(1, mm_doc::Interpolation::Hold),
        ],
    }
}

/// 種からレイヤーを作る。開始と長さは呼び出し側が決める。
pub fn build_layer(spec: &LayerSpec, p: &Project, start: i64, len: i64) -> Layer {
    let mut l = layer(&spec.id, start, 0, len);
    l.source = build_node(&spec.node, p);
    l.parent = spec.parent.and_then(|i| layer_id(p, i));
    l.attach_to = spec.attach.and_then(|i| cut_id(p, i));
    l.track_matte = spec
        .matte
        .and_then(|i| layer_id(p, i))
        .map(|layer| TrackMatte {
            layer,
            kind: MatteKind::Alpha,
            inverted: false,
        });
    if let Some(e) = &spec.effect {
        l.effects.push(Effect {
            id: Id::new(format!("{}-fx", spec.id)),
            node: build_node(e, p),
            enabled: true,
        });
    }
    if spec.mask {
        l.masks.push(mask_for(&spec.id));
    }
    if spec.audio {
        l.audio = Some(LayerAudio {
            role: "voice".to_owned(),
            gain_db: Param::constant(ParamValue::Number(0.0)),
            pan: Param::constant(ParamValue::Number(0.0)),
        });
    }
    if spec.remap {
        l.time_remap = Some(remap());
    }
    l
}

/// 種からカットを作る。
pub fn build_cut(spec: &CutSpec, p: &Project) -> mm_doc::Cut {
    let mut c = cut(&spec.id, spec.in_s, spec.in_s + spec.len, (1, 1));
    c.source = build_node(&spec.node, p);
    c.transition_in = spec.transition.as_ref().map(|t| Transition {
        node: build_node(t, p),
        duration: RationalTime::ZERO,
    });
    c.origin = spec.origin.and_then(|i| cut_id(p, i));
    c
}
