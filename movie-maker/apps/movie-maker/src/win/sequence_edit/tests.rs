use mm_doc::{Asset, Composition, Layer};
use mm_ops::{apply_transaction, Entry, OpContext};
use mm_types::{ColorSpace, ParamValue, Rational};

use super::*;

/// 試験用のプロジェクト。素材 `a`（10 秒）と、シーケンス `talk`（k1: 0-3 秒、k2: 3-4 秒、k3: 4-7 秒の素材）を
/// 持つシーケンスのレイヤー `l-seq` が 20 秒のコンポジション `main` に載る。
pub(crate) fn sample_project() -> Project {
    let fps = Rational::integer(30);
    let secs = RationalTime::seconds;
    let asset: AssetId = Id::new("a");
    let cut = |id: &str, a, b| Cut::new(id, Node::footage(&asset), secs(a), secs(b));
    let mut p = Project::new();
    p.assets.push(Asset {
        id: asset.clone(),
        path: "a.mp4".into(),
        content_hash: "h".into(),
        color_space: ColorSpace::bt709(),
        duration: secs(10),
        fps: Some(fps),
        timebase_offset: RationalTime::ZERO,
    });
    p.sequences.push(Sequence {
        id: Id::new("talk"),
        cuts: vec![cut("k1", 0, 3), cut("k2", 3, 4), cut("k3", 4, 7)],
        origin: None,
    });
    let node =
        Node::new(SEQUENCE_KIND, 1).with_param(SEQUENCE_PARAM, ParamValue::Text("talk".into()));
    p.compositions.push(
        Composition::new(Id::new("main"), 64, 36, fps, secs(20)).with_layers(vec![Layer::new(
            "l-seq",
            node,
            secs(0),
            secs(0),
            secs(20),
        )]),
    );
    p
}

fn ctx<'a>(p: &'a Project, cut: Option<&'a str>, playhead: f64) -> Ctx<'a> {
    Ctx {
        project: p,
        comp: &p.compositions[0],
        layer: None,
        cut,
        playhead,
    }
}

fn apply(p: &mut Project, op: Op) -> mm_ops::TxnOutcome {
    apply_transaction(p, &[op], &OpContext::new(Entry::WebView)).expect("適用できる")
}

fn cut_ids(p: &Project) -> Vec<String> {
    p.sequences[0]
        .cuts
        .iter()
        .map(|c| c.id.to_string())
        .collect()
}

/// 素材 `a` の 5 秒から 6 秒。
fn range(_: &Project) -> SourceRange {
    SourceRange {
        asset: Id::new("a"),
        in_point: RationalTime::seconds(5),
        out_point: RationalTime::seconds(6),
    }
}

#[test]
fn 印が無ければ素材の全体_印があればその範囲() {
    let mut p = sample_project();
    let a: AssetId = Id::new("a");
    let r = source_range(&p, &a).unwrap();
    assert_eq!(
        (r.in_point, r.out_point),
        (RationalTime::ZERO, RationalTime::seconds(10))
    );
    apply(
        &mut p,
        mm_ops::MarkIn {
            asset: a.clone(),
            at: Some(RationalTime::seconds(2)),
        }
        .into(),
    );
    apply(
        &mut p,
        mm_ops::MarkOut {
            asset: a.clone(),
            at: Some(RationalTime::seconds(4)),
        }
        .into(),
    );
    let r = source_range(&p, &a).unwrap();
    assert_eq!(
        (r.in_point, r.out_point),
        (RationalTime::seconds(2), RationalTime::seconds(4))
    );
}

#[test]
fn insert_は再生位置に近い境目へ入り_後ろがずれる() {
    let p = sample_project();
    let r = range(&p);
    let index_for = |t: f64| {
        let planned = plan_insert(&ctx(&p, None, t), &r, Id::new("new")).unwrap();
        let Op::Insert(i) = planned.op else { panic!() };
        i.index
    };
    assert_eq!(index_for(0.5), 0, "k1 の前半は k1 の前");
    assert_eq!(index_for(2.0), 1, "k1 の後半は k1 の後ろ");
    assert_eq!(index_for(3.1), 1, "k2 の前半は k2 の前");
    assert_eq!(index_for(15.0), 3, "末尾より後ろは末尾");
    assert_eq!(index_for(7.0), 3, "ちょうど末尾");
}

#[test]
fn insert_を適用すると_カットが増え_素材のその範囲が並び_取り消せる() {
    let mut p = sample_project();
    let before = p.clone();
    let planned = plan_insert(&ctx(&p, None, 3.2), &range(&p), Id::new("new")).unwrap();
    assert_eq!(planned.select, Some(Id::new("new")));
    let outcome = apply(&mut p, planned.op);
    assert_eq!(cut_ids(&p), ["k1", "new", "k2", "k3"]);
    let (_, cut) = p.cut(&Id::new("new")).unwrap();
    assert_eq!(
        (cut.in_point, cut.out_point),
        (RationalTime::seconds(5), RationalTime::seconds(6))
    );
    assert_eq!(cut.source.text_param("asset"), None, "素材は AssetRef");
    for op in outcome.inverse_ops().unwrap() {
        apply(&mut p, op);
    }
    assert_eq!(p, before, "逆操作で元に戻る");
}

#[test]
fn overwrite_は選んだカットを置き換え_選んでいなければ再生位置のカット() {
    let p = sample_project();
    let r = range(&p);
    let planned = plan_overwrite(&ctx(&p, Some("k3"), 0.5), &r, Id::new("new")).unwrap();
    let Op::Overwrite(o) = planned.op else {
        panic!()
    };
    assert_eq!(o.cut.as_str(), "k3");
    let planned = plan_overwrite(&ctx(&p, None, 3.5), &r, Id::new("new")).unwrap();
    let Op::Overwrite(o) = planned.op else {
        panic!()
    };
    assert_eq!(o.cut.as_str(), "k2");
    assert!(plan_overwrite(&ctx(&p, None, 15.0), &r, Id::new("new")).is_err());
}

#[test]
fn split_は再生位置で割り_カットの先頭では割らない() {
    let mut p = sample_project();
    let planned = plan_split(&ctx(&p, None, 1.0), Id::new("k1b")).unwrap();
    apply(&mut p, planned.op);
    assert_eq!(cut_ids(&p), ["k1", "k1b", "k2", "k3"]);
    let (_, first) = p.cut(&Id::new("k1")).unwrap();
    assert_eq!(first.out_point, RationalTime::seconds(1));
    assert!(plan_split(&ctx(&p, None, 0.0), Id::new("x")).is_err());
    assert!(plan_split(&ctx(&p, None, 15.0), Id::new("x")).is_err());
}

#[test]
fn lift_は隙間を残し_extract_は詰める() {
    let mut p = sample_project();
    let lift = plan_lift(&ctx(&p, Some("k2"), 0.0)).unwrap().op;
    apply(&mut p, lift);
    assert!(p.cut(&Id::new("k2")).unwrap().1.source.is_gap());
    assert_eq!(cut_ids(&p).len(), 3);
    let extract = plan_extract(&ctx(&p, Some("k2"), 0.0)).unwrap().op;
    apply(&mut p, extract);
    assert_eq!(cut_ids(&p), ["k1", "k3"]);
    assert!(plan_lift(&ctx(&p, None, 0.0)).is_err(), "選んでいない");
}

fn ids<'a>(layers: &'a [String], cuts: &'a [Vec<String>]) -> TimelineIds<'a> {
    TimelineIds { layers, cuts }
}

#[test]
fn ドラッグの編集は_ripple_roll_slip_slide_trim_layer_の操作になり_どれも取り消せる() {
    let layers = vec!["l-seq".to_owned()];
    let cuts = vec![vec!["k1".to_owned(), "k2".to_owned(), "k3".to_owned()]];
    let kinds = [
        EditKind::Ripple {
            item: 0,
            cut: 0,
            side: Side::Out,
        },
        EditKind::Ripple {
            item: 0,
            cut: 1,
            side: Side::In,
        },
        EditKind::Roll { item: 0, cut: 0 },
        EditKind::Slip { item: 0, cut: 2 },
        EditKind::Slide { item: 0, cut: 1 },
        EditKind::TrimLayer {
            item: 0,
            side: Side::Out,
        },
    ];
    for kind in kinds {
        let mut p = sample_project();
        let before = p.clone();
        let op = gesture_op(kind, -0.5, &ids(&layers, &cuts), &p.compositions[0].clone())
            .unwrap_or_else(|| panic!("{kind:?}"));
        let outcome = apply(&mut p, op);
        assert_ne!(p, before, "{kind:?} で変わる");
        for inverse in outcome.inverse_ops().unwrap() {
            apply(&mut p, inverse);
        }
        assert_eq!(p, before, "{kind:?} は取り消せる");
    }
}

#[test]
fn ripple_の出は後ろのカットをずらす() {
    let mut p = sample_project();
    let layers = vec!["l-seq".to_owned()];
    let cuts = vec![vec!["k1".to_owned(), "k2".to_owned(), "k3".to_owned()]];
    let kind = EditKind::Ripple {
        item: 0,
        cut: 0,
        side: Side::Out,
    };
    let op = gesture_op(kind, 0.5, &ids(&layers, &cuts), &p.compositions[0].clone()).unwrap();
    apply(&mut p, op);
    let placed = place_cuts(&p.sequences[0]).unwrap();
    assert_eq!(
        placed[1].range.start,
        RationalTime::new(7, 2).unwrap(),
        "k2 は 3.5 秒から"
    );
    assert_eq!(
        p.cut(&Id::new("k1")).unwrap().1.out_point,
        RationalTime::new(7, 2).unwrap()
    );
}

#[test]
fn 速さ_2_のレイヤーではカットの操作の量はシーケンスの時間軸で_2_倍_trim_layer_はそのまま() {
    let mut p = sample_project();
    p.compositions[0].layers[0].speed = Rational::integer(2);
    let comp = p.compositions[0].clone();
    let layers = vec!["l-seq".to_owned()];
    let cuts = vec![vec!["k1".to_owned(), "k2".to_owned(), "k3".to_owned()]];
    let half = RationalTime::new(1, 2).unwrap();
    let slip = gesture_op(
        EditKind::Slip { item: 0, cut: 0 },
        0.5,
        &ids(&layers, &cuts),
        &comp,
    );
    let Some(Op::Slip(slip)) = slip else {
        panic!("slip になる")
    };
    assert_eq!(
        slip.by,
        RationalTime::seconds(1),
        "画面の 0.5 秒 = シーケンスの 1 秒"
    );
    let trim = EditKind::TrimLayer {
        item: 0,
        side: Side::Out,
    };
    let Some(Op::TrimLayer(trim)) = gesture_op(trim, 0.5, &ids(&layers, &cuts), &comp) else {
        panic!("trimLayer になる")
    };
    assert_eq!(trim.by, half, "レイヤーの端はコンポジションの時間軸");
}

#[test]
fn 再生位置がフレームの後半でも_split_は表示中のフレームで割る() {
    let p = sample_project();
    // 1 秒 + 0.9 フレーム。表示は 30 フレーム目なので、割る位置は 1 秒。
    let at = 1.0 + 0.9 / 30.0;
    let planned = plan_split(&ctx(&p, None, at), Id::new("k1b")).unwrap();
    let Op::Split(split) = planned.op else {
        panic!("split になる")
    };
    assert_eq!(split.at, RationalTime::seconds(1));
}

#[test]
fn 量が_0_フレームか_対応が無ければ操作にしない() {
    let p = sample_project();
    let comp = p.compositions[0].clone();
    let layers = vec!["l-seq".to_owned()];
    let cuts = vec![vec!["k1".to_owned()]];
    let kind = EditKind::Slip { item: 0, cut: 0 };
    assert!(gesture_op(kind, 0.001, &ids(&layers, &cuts), &comp).is_none());
    let gone = EditKind::Slip { item: 0, cut: 5 };
    assert!(gesture_op(gone, 1.0, &ids(&layers, &cuts), &comp).is_none());
}

#[test]
fn 新しいカットの_id_は既存と重ならない() {
    let p = sample_project();
    let state = EditState::default();
    let a = state.fresh_cut_id(&p);
    let b = state.fresh_cut_id(&p);
    assert_ne!(a, b);
    assert!(p.cut(&a).is_err());
}

/// 編集の試験と実機の確認に使うフィクスチャ。
pub(crate) const EDIT_FIXTURE: &str = "../../fixtures/projects/stage3-edit/project.json";

#[test]
fn 編集用のフィクスチャは隙間を含む_3_カットのシーケンスのレイヤーを持つ() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(EDIT_FIXTURE);
    let p = crate::win::project::open(&path).expect("開ける").project;
    let seq = &p.sequences[0];
    let gaps: Vec<_> = seq.cuts.iter().map(|c| c.source.is_gap()).collect();
    assert_eq!(gaps, [false, true, false]);
    let comp = p.composition(&Id::new("main")).unwrap();
    assert!(comp.layers.iter().any(|l| l.source.kind == SEQUENCE_KIND));
    assert_eq!(p.assets.len(), 2, "Source で選べる素材が 2 本");
    let ctx = Ctx {
        project: &p,
        comp,
        layer: None,
        cut: Some("c3"),
        playhead: 0.5,
    };
    let range = source_range(&p, &Id::new("take2")).unwrap();
    assert!(plan_overwrite(&ctx, &range, Id::new("n")).is_ok());
}
