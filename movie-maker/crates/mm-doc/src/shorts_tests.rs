use super::*;
use crate::sequence::{Sequence, SequenceOrigin};
use crate::Node;
use mm_types::Id;

fn secs(n: i64) -> RationalTime {
    RationalTime::seconds(n)
}

fn cut(id: &str, a: i64, b: i64) -> Cut {
    Cut::new(id, Node::new("solid", 1), secs(a), secs(b))
}

fn linked(id: &str, origin: &str, a: i64, b: i64) -> Cut {
    let mut c = cut(id, a, b);
    c.origin = Some(Id::new(origin));
    c
}

/// 本編 `main`（m1, m2）と、それを参照するショート `short`（s1 → m1, s2 → m2）。
fn project(relation: OriginRelation) -> Project {
    let mut p = Project::new();
    p.sequences.push(Sequence {
        id: Id::new("main"),
        cuts: vec![cut("m1", 0, 4), cut("m2", 10, 12)],
        origin: None,
    });
    p.sequences.push(Sequence {
        id: Id::new("short"),
        cuts: vec![linked("s1", "m1", 0, 4), linked("s2", "m2", 10, 12)],
        origin: Some(SequenceOrigin {
            sequence: Id::new("main"),
            relation,
        }),
    });
    p
}

fn short_cut<'a>(p: &'a Project, id: &str) -> &'a Cut {
    p.cut(&Id::new(id)).unwrap().1
}

#[test]
fn 元のカットを直すと参照のカットに写る() {
    let mut p = project(OriginRelation::Reference);
    let before = p.reference_snapshot();
    p.cut_mut(&Id::new("m1")).unwrap().out_point = secs(3);
    p.cut_mut(&Id::new("m2")).unwrap().enabled = false;
    p.sync_references(&before).unwrap();
    assert_eq!(short_cut(&p, "s1").out_point, secs(3));
    assert!(!short_cut(&p, "s2").enabled);
}

#[test]
fn 上書きした所は写さず_上書きしていない所は写す() {
    let mut p = project(OriginRelation::Reference);
    {
        let s1 = p.cut_mut(&Id::new("s1")).unwrap();
        s1.overrides.out_point = true;
        s1.out_point = secs(2);
    }
    let before = p.reference_snapshot();
    let m1 = p.cut_mut(&Id::new("m1")).unwrap();
    m1.in_point = secs(1);
    m1.out_point = secs(3);
    p.sync_references(&before).unwrap();
    let s1 = short_cut(&p, "s1");
    assert_eq!((s1.in_point, s1.out_point), (secs(1), secs(2)));
}

#[test]
fn 参照のカットの上書きしていない所を直接直すと拒否する() {
    let mut p = project(OriginRelation::Reference);
    let before = p.reference_snapshot();
    p.cut_mut(&Id::new("s1")).unwrap().out_point = secs(2);
    assert!(p.sync_references(&before).is_err());

    let mut p = project(OriginRelation::Reference);
    let before = p.reference_snapshot();
    p.cut_mut(&Id::new("s1")).unwrap().enabled = false;
    assert!(p.sync_references(&before).is_err());
}

#[test]
fn 上書きを外すと元の値に戻る() {
    let mut p = project(OriginRelation::Reference);
    {
        let s1 = p.cut_mut(&Id::new("s1")).unwrap();
        s1.overrides.out_point = true;
        s1.out_point = secs(2);
    }
    let before = p.reference_snapshot();
    p.cut_mut(&Id::new("s1")).unwrap().overrides.out_point = false;
    p.sync_references(&before).unwrap();
    assert_eq!(short_cut(&p, "s1").out_point, secs(4));
}

#[test]
fn 足した参照のカットは元と同じ値なら通り_違えば拒否する() {
    let mut p = project(OriginRelation::Reference);
    let before = p.reference_snapshot();
    p.sequence_mut(&Id::new("short"))
        .unwrap()
        .cuts
        .push(linked("s3", "m1", 0, 4));
    p.sync_references(&before).unwrap();

    let mut p = project(OriginRelation::Reference);
    let before = p.reference_snapshot();
    p.sequence_mut(&Id::new("short"))
        .unwrap()
        .cuts
        .push(linked("s3", "m1", 1, 4));
    assert!(p.sync_references(&before).is_err());
}

#[test]
fn 複製のカットは写さない() {
    let mut p = project(OriginRelation::Fork);
    let before = p.reference_snapshot();
    p.cut_mut(&Id::new("m1")).unwrap().out_point = secs(3);
    p.cut_mut(&Id::new("s2")).unwrap().out_point = secs(11);
    p.sync_references(&before).unwrap();
    assert_eq!(short_cut(&p, "s1").out_point, secs(4));
    assert_eq!(short_cut(&p, "s2").out_point, secs(11));
}

#[test]
fn ショートのショートは元のショートを写した後に写す() {
    let mut p = project(OriginRelation::Reference);
    p.sequences.push(Sequence {
        id: Id::new("a-shorter"),
        cuts: vec![linked("t1", "s1", 0, 4)],
        origin: Some(SequenceOrigin {
            sequence: Id::new("short"),
            relation: OriginRelation::Reference,
        }),
    });
    let before = p.reference_snapshot();
    p.cut_mut(&Id::new("m1")).unwrap().out_point = secs(3);
    p.sync_references(&before).unwrap();
    assert_eq!(short_cut(&p, "t1").out_point, secs(3));
}

#[test]
fn 本編のカットごとに使うショートを引く() {
    let mut p = project(OriginRelation::Reference);
    p.cut_mut(&Id::new("s2")).unwrap().overrides.in_point = true;
    let uses = p.short_uses(&Id::new("main"));
    assert_eq!(uses.len(), 2);
    let m2 = &uses[&Id::new("m2")];
    assert_eq!(m2[0].short.as_str(), "short");
    assert_eq!(m2[0].cut.as_str(), "s2");
    assert!(m2[0].overrides.in_point);
    assert!(p.short_uses(&Id::new("short")).is_empty());
}

#[test]
fn 複製の元が変わったカットと消えたカットを返す() {
    let mut p = project(OriginRelation::Fork);
    for (s, m) in [("s1", "m1"), ("s2", "m2")] {
        let base = CutBase::of(short_cut(&p, m));
        p.cut_mut(&Id::new(s)).unwrap().origin_base = Some(base);
    }
    assert!(p.origin_changes(&Id::new("short")).unwrap().is_empty());
    p.cut_mut(&Id::new("m1")).unwrap().out_point = secs(3);
    p.sequence_mut(&Id::new("main")).unwrap().cuts.remove(1);
    let changes = p.origin_changes(&Id::new("short")).unwrap();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].cut.as_str(), "s1");
    assert_eq!(changes[0].fields, vec!["out"]);
    assert!(changes[1].removed);
}
