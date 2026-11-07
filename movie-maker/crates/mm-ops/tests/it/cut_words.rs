//! `cutWords` の振る舞い。単語を消し、その範囲をシーケンスのカットから抜いて詰め、1 回の逆操作で戻る。

use crate::common;

use common::{canon, project, quarters, secs};
use mm_doc::{Cut, Node, Project};
use mm_ops::*;
use mm_types::{Id, RationalTime};

fn apply(p: &mut Project, op: impl Into<Op>) -> OpResult<Applied> {
    op.into().apply(p, &OpContext::human())
}

/// s1 の後ろに、素材 a1 の [1, 4) と [6, 8) のカット f1 と f2 を足したプロジェクトのデータ。
fn with_footage() -> Project {
    let mut p = project();
    let a1 = Id::new("a1");
    let cuts = &mut p.sequences[0].cuts;
    cuts.push(Cut::new("f1", Node::footage(&a1), secs(1), secs(4)));
    cuts.push(Cut::new("f2", Node::footage(&a1), secs(6), secs(8)));
    p.validate().unwrap();
    p
}

fn range(from: RationalTime, to: RationalTime) -> SourceRange {
    SourceRange {
        in_point: from,
        out_point: to,
    }
}

fn cut_words(words: &[&str], ranges: Vec<SourceRange>) -> CutWords {
    CutWords {
        asset: Id::new("a1"),
        sequence: Id::new("s1"),
        words: words.iter().map(|w| Id::new(*w)).collect(),
        ranges,
        restore: false,
        splices: Vec::new(),
    }
}

/// s1 のカットの ID と素材の範囲。
fn spans(p: &Project) -> Vec<(String, RationalTime, RationalTime)> {
    p.sequences[0]
        .cuts
        .iter()
        .map(|c| (c.id.to_string(), c.in_point, c.out_point))
        .collect()
}

fn span(id: &str, from: RationalTime, to: RationalTime) -> (String, RationalTime, RationalTime) {
    (id.to_owned(), from, to)
}

#[test]
fn カットの中ほどの範囲は割って間を抜き_1_回の逆操作で戻る() {
    let mut p = with_footage();
    let before = canon(&p);
    let op = cut_words(&["w1", "w2"], vec![range(quarters(6), quarters(10))]);
    let applied = apply(&mut p, op).unwrap();
    let got = spans(&p);
    assert_eq!(got[4], span("f1", secs(1), quarters(6)));
    assert_eq!(got[5], span("f1-1", quarters(10), secs(4)));
    assert_eq!(got[6], span("f2", secs(6), secs(8)));
    assert!(p.transcript_edits.iter().all(|e| e.deleted));
    assert_eq!(p.transcript_edits.len(), 2);
    assert!(applied.touches.iter().any(|t| t == "s1"));
    let redo = apply(&mut p, applied.inverse).unwrap();
    assert_eq!(canon(&p), before);
    apply(&mut p, redo.inverse).unwrap();
    assert_eq!(spans(&p)[5], span("f1-1", quarters(10), secs(4)));
}

#[test]
fn 複数のカットにまたがる範囲は_端を詰め_覆ったカットを抜く() {
    let mut p = with_footage();
    apply(&mut p, cut_words(&["w1"], vec![range(secs(3), secs(9))])).unwrap();
    let got = spans(&p);
    assert_eq!(got.len(), 5, "f2 は範囲に覆われて抜ける: {got:?}");
    assert_eq!(got[4], span("f1", secs(1), secs(3)));
}

#[test]
fn 重なるか接する範囲は_1_つにまとめる() {
    let mut p = with_footage();
    let ranges = vec![
        range(quarters(10), quarters(14)),
        range(quarters(6), quarters(10)),
    ];
    apply(&mut p, cut_words(&["w1"], ranges)).unwrap();
    let got = spans(&p);
    assert_eq!(got[4], span("f1", secs(1), quarters(6)));
    assert_eq!(got[5], span("f1-1", quarters(14), secs(4)));
}

#[test]
fn カットの外の範囲は単語を消すだけ() {
    let mut p = with_footage();
    let cuts_before = spans(&p);
    apply(&mut p, cut_words(&["w1"], vec![range(secs(4), secs(6))])).unwrap();
    assert_eq!(spans(&p), cuts_before);
    assert!(p.transcript_edits[0].deleted);
}

#[test]
fn 既に消した単語は通し_何も変わらなければ拒否する() {
    let mut p = with_footage();
    apply(&mut p, cut_words(&["w1"], vec![])).unwrap();
    let applied = apply(&mut p, cut_words(&["w1"], vec![range(secs(1), secs(2))])).unwrap();
    let CutWords { words, .. } = match applied.inverse {
        Op::CutWords(c) => *c,
        other => panic!("逆操作が cutWords でない: {other:?}"),
    };
    assert!(words.is_empty(), "変えていない単語を戻さない");
    assert!(apply(&mut p, cut_words(&["w1"], vec![range(secs(4), secs(5))])).is_err());
}

#[test]
fn 割った後ろの_id_は空いている番号を取る() {
    let mut p = with_footage();
    let a1 = Id::new("a1");
    p.sequences[0]
        .cuts
        .push(Cut::new("f1-1", Node::footage(&a1), secs(10), secs(11)));
    apply(&mut p, cut_words(&["w1"], vec![range(secs(2), secs(3))])).unwrap();
    assert_eq!(spans(&p)[5].0, "f1-2");
}

#[test]
fn 指した_sequence_のカットだけを切り_ほかの_sequence_の同じ素材は残す() {
    let mut p = with_footage();
    let s2_before = p.sequence(&Id::new("s2")).unwrap().cuts.clone();
    assert!(
        s2_before.iter().any(|c| c.in_point < secs(2)),
        "s2 に a1 の [0, 2) がある"
    );
    let applied = apply(&mut p, cut_words(&["w1"], vec![range(secs(1), secs(2))])).unwrap();
    assert_eq!(p.sequence(&Id::new("s2")).unwrap().cuts, s2_before);
    assert_eq!(spans(&p)[4], span("f1", secs(2), secs(4)));
    assert!(!applied.touches.iter().any(|t| t == "s2" || t == "kf"));
}

#[test]
fn 範囲の_in_が_out_より前でなければ拒否する() {
    let mut p = with_footage();
    let before = canon(&p);
    assert!(apply(&mut p, cut_words(&["w1"], vec![range(secs(2), secs(2))])).is_err());
    assert_eq!(canon(&p), before);
}
