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
    let mut layer = Layer::new(
        LAYER_ID,
        node,
        RationalTime::ZERO,
        RationalTime::ZERO,
        duration,
    );
    let host = host_of(&project, &rt.composition, sequence);
    layer.framing = host.map(|(_, l)| l.framing.clone()).unwrap_or_default();
    layer.audio = host.and_then(|(_, l)| l.audio.clone());
    let mixer = host.and_then(|(c, _)| c.mixer.clone());
    project.compositions.push(
        Composition::new(id.clone(), rt.width, rt.height, rt.fps, duration)
            .with_layers(vec![layer])
            .with_mixer_opt(mixer),
    );
    let t = project
        .render_targets
        .iter_mut()
        .find(|t| t.id == *target)
        .ok_or_else(|| MmError::NotFound(format!("render target {target}")))?;
    t.composition = id.clone();
    Ok((project, id))
}

/// 見せ方（再フレーミングと字幕の置き場所）と音（レイヤーの音の設定とミキサー）を引き継ぐ、`sequence` を置いたレイヤー。
/// レンダーターゲットが描いていたコンポジションに 1 つあればそれを、無ければプロジェクト全体で 1 つだけのものを使う。
/// ショートの見せ方はショートのコンポジションのレイヤーにあり、どれを引き継ぐか決まらないときは何も引き継がない。
fn host_of<'a>(
    project: &'a Project,
    current: &CompositionId,
    sequence: &SequenceId,
) -> Option<(&'a Composition, &'a Layer)> {
    let places = |c: &'a Composition| -> Vec<(&'a Composition, &'a Layer)> {
        c.layers
            .iter()
            .filter(|l| {
                l.source.kind == SEQUENCE_KIND
                    && l.source.text_param(SEQUENCE_PARAM) == Some(sequence.as_str())
            })
            .map(|l| (c, l))
            .collect()
    };
    let only = |v: Vec<(&'a Composition, &'a Layer)>| (v.len() == 1).then(|| v[0]);
    let in_current = project
        .compositions
        .iter()
        .find(|c| c.id == *current)
        .map(places)
        .and_then(only);
    in_current.or_else(|| only(project.compositions.iter().flat_map(places).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_doc::{Cut, CutReframe, Framing, Param, RenderTarget, Sequence, Transform};
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

    /// `s` を置き、カット `a` を再フレーミングしたレイヤーを持つコンポジション。
    fn short_comp(id: &str, x: f64) -> Composition {
        let node =
            Node::new(SEQUENCE_KIND, 1).with_param(SEQUENCE_PARAM, ParamValue::Text("s".into()));
        let mut layer = Layer::new(
            "l",
            node,
            RationalTime::ZERO,
            RationalTime::ZERO,
            RationalTime::seconds(5),
        );
        let transform = Transform {
            position: Param::constant(ParamValue::Vec2([x, 0.0])),
            ..Transform::default()
        };
        layer.framing.cuts.push(CutReframe {
            cut: Id::new("a"),
            transform,
        });
        Composition::new(
            Id::new(id),
            360,
            640,
            Rational::integer(30),
            RationalTime::seconds(5),
        )
        .with_layers(vec![layer])
    }

    fn exported_framing(p: Project) -> Framing {
        let (p, comp) = retarget_to_sequence(p, &Id::new("t"), &Id::new("s")).unwrap();
        p.composition(&comp).unwrap().layers[0].framing.clone()
    }

    #[test]
    fn ショートの再フレーミングを引き継ぐ() {
        let mut p = project();
        p.compositions.push(short_comp("short", 40.0));
        let framing = exported_framing(p);
        assert_eq!(framing.cuts.len(), 1);
        assert_eq!(framing.cuts[0].cut.as_str(), "a");
    }

    #[test]
    fn 置き場所が複数ならレンダーターゲットのコンポジションのものを引き継ぐ() {
        let mut p = project();
        p.compositions.push(short_comp("short-a", 10.0));
        p.compositions.push(short_comp("short-b", 99.0));
        assert_eq!(exported_framing(p.clone()), Framing::default());
        p.render_targets[0].composition = Id::new("short-b");
        let framing = exported_framing(p);
        let expected = Param::constant(ParamValue::Vec2([99.0, 0.0]));
        assert_eq!(framing.cuts[0].transform.position, expected);
    }

    #[test]
    fn 置いたレイヤーの音の設定とコンポジションのミキサーを引き継ぐ() {
        let mut p = project();
        let mut comp = short_comp("short", 0.0);
        comp.mixer = Some(mm_doc::CompMixer {
            master_db: -3.0,
            ..mm_doc::CompMixer::default()
        });
        comp.layers[0].audio = Some(mm_doc::LayerAudio {
            gain_db: Param::constant(ParamValue::Number(-6.0)),
            mute: true,
            ..mm_doc::LayerAudio::new("voice")
        });
        let (want_mixer, want_audio) = (comp.mixer.clone(), comp.layers[0].audio.clone());
        p.compositions.push(comp);
        let (p, id) = retarget_to_sequence(p, &Id::new("t"), &Id::new("s")).unwrap();
        let c = p.composition(&id).unwrap();
        assert_eq!(c.mixer, want_mixer);
        assert_eq!(c.layers[0].audio, want_audio);
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
