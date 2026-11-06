//! sequence をそのまま書き出す。sequence を 1 つのレイヤーに置いただけの書き出し用のコンポジションを足す。
//! レンダラーは sequence のノードを展開して描く（カット、トランジション、音）ので、書き出しは普通のコンポジションと同じ経路になる。

use mm_doc::{Composition, Layer, Node, Project, SequenceId, SEQUENCE_KIND, SEQUENCE_PARAM};
use mm_types::{CompositionId, Id, MmError, ParamValue, RationalTime, RenderTargetId, Result};

/// 書き出し用のコンポジションの ID の接頭辞。
const COMPOSITION_PREFIX: &str = "export-sequence-";
/// sequence を置くレイヤーの ID。
const LAYER_ID: &str = "export-sequence-layer";

/// `project` に、`sequence` を全長で置いたコンポジションを足し、`target` がそれを描くようにして返す。
/// 大きさと fps はレンダーターゲットのものを使う。
pub(crate) fn retarget_to_sequence(
    mut project: Project,
    target: &RenderTargetId,
    sequence: &SequenceId,
) -> Result<(Project, CompositionId)> {
    let duration = mm_eval::sequence_duration(project.sequence(sequence)?)?;
    if duration <= RationalTime::ZERO {
        return Err(MmError::InvalidArgument(format!(
            "sequence {sequence} に有効なカットが無い"
        )));
    }
    let id: CompositionId = Id::new(format!("{COMPOSITION_PREFIX}{sequence}"));
    if project.composition(&id).is_ok() {
        return Err(MmError::InvalidArgument(format!(
            "コンポジション {id} が既にある"
        )));
    }
    let rt = project.render_target(target)?.clone();
    let node = Node::new(SEQUENCE_KIND, 1)
        .with_param(SEQUENCE_PARAM, ParamValue::Text(sequence.to_string()));
    project.compositions.push(Composition {
        id: id.clone(),
        width: rt.width,
        height: rt.height,
        fps: rt.fps,
        duration,
        layers: vec![Layer::new(
            LAYER_ID,
            node,
            RationalTime::ZERO,
            RationalTime::ZERO,
            duration,
        )],
        markers: Vec::new(),
    });
    let t = project
        .render_targets
        .iter_mut()
        .find(|t| t.id == *target)
        .ok_or_else(|| MmError::NotFound(format!("render target {target}")))?;
    t.composition = id.clone();
    Ok((project, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_doc::{Cut, RenderTarget, Sequence};
    use mm_types::Rational;

    fn project() -> Project {
        let node = Node::new("solid", 1);
        let seq = Sequence {
            id: Id::new("s"),
            cuts: vec![
                Cut::new(
                    "a",
                    node.clone(),
                    RationalTime::ZERO,
                    RationalTime::seconds(2),
                ),
                Cut::new("b", node, RationalTime::ZERO, RationalTime::seconds(3)),
            ],
            origin: None,
        };
        let mut p = Project::new();
        p.sequences.push(seq);
        p.render_targets.push(RenderTarget {
            id: Id::new("t"),
            composition: Id::new("none"),
            width: 640,
            height: 360,
            fps: Rational::integer(30),
            codec: "sample".into(),
            output_path: "out.mp4".into(),
            loudness_lufs: None,
            delivery: Default::default(),
        });
        p
    }

    #[test]
    fn sequence_の全長を置いたコンポジションを描くようになる() {
        let (p, comp) = retarget_to_sequence(project(), &Id::new("t"), &Id::new("s")).unwrap();
        let c = p.composition(&comp).unwrap();
        assert_eq!(c.duration, RationalTime::seconds(5));
        assert_eq!((c.width, c.height), (640, 360));
        assert_eq!(p.render_target(&Id::new("t")).unwrap().composition, comp);
        assert_eq!(c.layers[0].source.kind, SEQUENCE_KIND);
    }

    #[test]
    fn 有効なカットの無い_sequence_は拒否する() {
        let mut p = project();
        p.sequences[0]
            .cuts
            .iter_mut()
            .for_each(|c| c.enabled = false);
        assert!(retarget_to_sequence(p, &Id::new("t"), &Id::new("s")).is_err());
        assert!(retarget_to_sequence(project(), &Id::new("t"), &Id::new("x")).is_err());
    }
}
