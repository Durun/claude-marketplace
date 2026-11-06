//! 文字起こしに無い単語への修正は書く前に拒否する。

use mm_doc::{Asset, Project};
use mm_ops::{AddAsset, DeleteWords, FixTranscript, OpContext, OpError, Transaction};
use mm_store::{word_id, Store, TranscriptInput, WordInput};
use mm_types::{ColorSpace, Id, RationalTime};

use crate::common::{actor, NOW};

const HASH: &str = "word-check-hash";

fn store_with_words(dir: &std::path::Path, with_transcript: bool) -> Store {
    let mut s = Store::create(dir, &Project::new(), NOW).unwrap();
    let asset = Asset {
        id: Id::new("a1"),
        path: "a.wav".into(),
        content_hash: HASH.into(),
        color_space: ColorSpace::bt709(),
        duration: RationalTime::seconds(10),
        fps: None,
        timebase_offset: RationalTime::ZERO,
    };
    let add = Transaction::new("素材", vec![AddAsset { asset, index: None }.into()]);
    s.apply_txn(add, &OpContext::human(), actor(), NOW, None)
        .unwrap();
    if with_transcript {
        let words = (0..3)
            .map(|i| WordInput {
                segment: 0,
                start: RationalTime::seconds(i),
                end: RationalTime::seconds(i + 1),
                text: format!("語{i}"),
            })
            .collect();
        let input = TranscriptInput {
            producer: "t".into(),
            language: "ja".into(),
            words,
        };
        s.put_transcript(&Id::new("a1"), HASH, &input).unwrap();
    }
    s
}

fn apply(s: &mut Store, op: impl Into<mm_ops::Op>) -> Result<(), OpError> {
    let txn = Transaction::new("テスト", vec![op.into()]);
    s.apply_txn(txn, &OpContext::human(), actor(), NOW, None)
        .map(|_| ())
}

fn delete(word: mm_doc::WordId) -> DeleteWords {
    DeleteWords {
        asset: Id::new("a1"),
        words: vec![word],
        restore: false,
    }
}

#[test]
fn 文字起こしに無い単語への削除と直しは拒否され_記録も増えない() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = store_with_words(tmp.path(), true);
    let version = s.doc_version();
    let ghost = word_id(HASH, RationalTime::seconds(99), 1);

    assert!(apply(&mut s, delete(ghost.clone())).is_err());
    let fix = FixTranscript {
        asset: Id::new("a1"),
        word: ghost,
        text: Some("幽霊".into()),
    };
    assert!(apply(&mut s, fix).is_err());
    assert_eq!(s.doc_version(), version);
    assert!(s.project().transcript_edits.is_empty());

    let real = word_id(HASH, RationalTime::seconds(1), 1);
    apply(&mut s, delete(real)).unwrap();
    assert_eq!(s.project().transcript_edits.len(), 1);
}

#[test]
fn 文字起こしの無い素材への直しも拒否する() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = store_with_words(tmp.path(), false);
    let word = word_id(HASH, RationalTime::seconds(1), 1);
    assert!(apply(&mut s, delete(word)).is_err());
}

#[test]
fn 取り消しは単語が取り込み直しで変わっても止まらない() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = store_with_words(tmp.path(), true);
    let word = word_id(HASH, RationalTime::seconds(1), 1);
    apply(&mut s, delete(word)).unwrap();
    // 取り込み直しで本文が変わり、直した単語が無くなっても、取り消しのスタックは進められる。
    let input = TranscriptInput {
        producer: "t".into(),
        language: "ja".into(),
        words: vec![WordInput {
            segment: 0,
            start: RationalTime::seconds(7),
            end: RationalTime::seconds(8),
            text: "別".into(),
        }],
    };
    s.put_transcript(&Id::new("a1"), HASH, &input).unwrap();
    assert!(s
        .undo_last(&OpContext::human(), actor(), NOW)
        .unwrap()
        .is_some());
}
