//! マグネティックシーケンスのカットの配置。有効なカットを先頭から詰め、位置は長さの和で決まる。

use mm_doc::{Cut, CutId, Framing, Param, Sequence, Transform};
use mm_types::{ParamValue, RationalTime, Result, TimeRange};

use crate::transform::evaluate_transform;

/// 配置したカット。`range` は sequence の時間軸。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedCut {
    pub id: CutId,
    /// `Sequence.cuts` の中の番号（無効なカットも数える）。
    pub index: usize,
    pub range: TimeRange,
}

/// 有効なカットを先頭から隙間なく並べる。無効なカットはスキップする。
/// トランジションは境目をまたいで掛けるもので、配置を変えない。
pub fn place_cuts(seq: &Sequence) -> Result<Vec<PlacedCut>> {
    let mut out = Vec::with_capacity(seq.cuts.len());
    let mut head = RationalTime::ZERO;
    for (index, cut) in seq.cuts.iter().enumerate() {
        if !cut.enabled {
            continue;
        }
        let range = TimeRange::from_start_duration(head, cut.duration()?)?;
        head = range.end;
        out.push(PlacedCut {
            id: cut.id.clone(),
            index,
            range,
        });
    }
    Ok(out)
}

/// シーケンス全体の長さ（有効なカットの長さの和）。
pub fn sequence_duration(seq: &Sequence) -> Result<RationalTime> {
    seq.cuts
        .iter()
        .filter(|c| c.enabled)
        .try_fold(RationalTime::ZERO, |acc, c| acc.checked_add(c.duration()?))
}

/// 時刻 `t` にあるカット。隙間が無いので、範囲の中なら必ず 1 つ。
pub fn cut_at(placed: &[PlacedCut], t: RationalTime) -> Option<&PlacedCut> {
    let i = placed.partition_point(|p| p.range.start <= t);
    placed[..i].last().filter(|p| p.range.contains(t))
}

/// 配置したカットの sequence の時刻を、そのカットの素材の時刻に変換する。
pub fn cut_source_time(cut: &Cut, placed: &PlacedCut, t: RationalTime) -> Result<RationalTime> {
    t.checked_sub(placed.range.start)?
        .scale(cut.speed)?
        .checked_add(cut.in_point)
}

/// カットの再フレーミング（`Framing.cuts`）を sequence の時刻 `t` で評価し、定数の変形にする。無ければ `None`。
/// キーフレームは素材の時刻で引く（被写体の位置は素材の時刻で決まる）。
pub fn reframe_at(
    framing: &Framing,
    cut: &Cut,
    placed: &PlacedCut,
    t: RationalTime,
) -> Result<Option<Transform>> {
    let Some(reframe) = framing.reframe(&cut.id) else {
        return Ok(None);
    };
    let e = evaluate_transform(&reframe.transform, cut_source_time(cut, placed, t)?);
    Ok(Some(Transform {
        position: Param::constant(ParamValue::Vec2(e.position)),
        scale: Param::constant(ParamValue::Vec2(e.scale)),
        rotation: Param::constant(ParamValue::Number(e.rotation_deg)),
        anchor: e.anchor.map(|a| Param::constant(ParamValue::Vec2(a))),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_doc::Node;
    use mm_types::{Id, Rational};
    use proptest::prelude::*;

    fn cut(id: &str, len: i64, enabled: bool) -> Cut {
        let mut c = Cut::new(
            id,
            Node::new("footage", 1),
            RationalTime::seconds(10),
            RationalTime::seconds(10 + len),
        );
        c.enabled = enabled;
        c
    }

    fn seq(cuts: Vec<Cut>) -> Sequence {
        Sequence {
            id: Id::new("s"),
            cuts,
            origin: None,
        }
    }

    #[test]
    fn 無効なカットはスキップして詰める() {
        let s = seq(vec![
            cut("a", 2, true),
            cut("b", 5, false),
            cut("c", 3, true),
        ]);
        let placed = place_cuts(&s).unwrap();
        assert_eq!(placed.len(), 2);
        assert_eq!(placed[1].index, 2);
        assert_eq!(placed[1].range.start, RationalTime::seconds(2));
        assert_eq!(sequence_duration(&s).unwrap(), RationalTime::seconds(5));
        assert_eq!(
            cut_at(&placed, RationalTime::seconds(2))
                .unwrap()
                .id
                .as_str(),
            "c"
        );
        assert!(cut_at(&placed, RationalTime::seconds(5)).is_none());
        assert!(cut_at(&placed, RationalTime::seconds(-1)).is_none());
    }

    #[test]
    fn カットの中の時刻は速さで素材に変換する() {
        let mut c = cut("a", 4, true);
        c.speed = Rational::integer(2);
        let s = seq(vec![cut("z", 1, true), c.clone()]);
        let placed = place_cuts(&s).unwrap();
        assert_eq!(
            placed[1].range.duration().unwrap(),
            RationalTime::seconds(2)
        );
        assert_eq!(
            cut_source_time(&c, &placed[1], RationalTime::seconds(2)).unwrap(),
            RationalTime::seconds(12)
        );
    }

    #[test]
    fn 再フレーミングのキーフレームは素材の時刻で引く() {
        use mm_doc::{CutReframe, Interpolation, Keyframe};
        let c = cut("a", 4, true);
        let s = seq(vec![cut("z", 2, true), c.clone()]);
        let placed = place_cuts(&s).unwrap();
        let mut transform = Transform::default();
        transform.position.keyframes = vec![
            Keyframe::new(
                RationalTime::seconds(10),
                ParamValue::Vec2([0.0, 0.0]),
                Interpolation::Linear,
            ),
            Keyframe::new(
                RationalTime::seconds(14),
                ParamValue::Vec2([40.0, 0.0]),
                Interpolation::Linear,
            ),
        ];
        let framing = Framing {
            cuts: vec![CutReframe {
                cut: c.id.clone(),
                transform,
            }],
            caption: None,
        };
        // sequence の 3 秒はカット a の 1 秒目、素材の 11 秒。
        let at = reframe_at(&framing, &c, &placed[1], RationalTime::seconds(3))
            .unwrap()
            .unwrap();
        assert_eq!(at.position.value, ParamValue::Vec2([10.0, 0.0]));
        assert!(
            reframe_at(&framing, &s.cuts[0], &placed[0], RationalTime::ZERO)
                .unwrap()
                .is_none()
        );
    }

    proptest! {
        #[test]
        fn 配置は重ならず隙間なく長さの和になる(lens in prop::collection::vec((1i64..100, any::<bool>()), 0..20)) {
            let cuts = lens
                .iter()
                .enumerate()
                .map(|(i, (len, on))| cut(&format!("c{i}"), *len, *on))
                .collect();
            let s = seq(cuts);
            let placed = place_cuts(&s).unwrap();
            let mut head = RationalTime::ZERO;
            for p in &placed {
                prop_assert_eq!(p.range.start, head);
                prop_assert!(p.range.end > p.range.start);
                head = p.range.end;
            }
            prop_assert_eq!(head, sequence_duration(&s).unwrap());
        }
    }
}
