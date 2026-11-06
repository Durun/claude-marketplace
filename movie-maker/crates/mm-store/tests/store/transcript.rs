//! 文字起こしの本文と全文検索、プロジェクトのデータの修正との同期、整理の行の保存。

use mm_doc::{Asset, Project, Rating, Topic};
use mm_ops::{AddAsset, AddTopic, DeleteWords, FixTranscript, Op, OpContext, Transaction};
use mm_store::{word_id, Store, TranscriptInput, WordInput};
use mm_types::{AssetId, ColorSpace, Id, RationalTime};

use crate::common::{actor, NOW};

const HASH: &str = "ca152c0602faf08d897d38f3";

fn t(num: i128, den: u32) -> RationalTime {
    RationalTime::new(num, den).unwrap()
}

fn asset() -> Asset {
    Asset {
        id: Id::new("a1"),
        path: "speech.wav".into(),
        content_hash: HASH.into(),
        color_space: ColorSpace::bt709(),
        duration: RationalTime::seconds(6),
        fps: None,
        timebase_offset: RationalTime::ZERO,
    }
}

/// フィクスチャの文字起こし（`fixtures/ingest/transcript-ja.json`）の先頭と同じ切れ方。「編集」が 2 つの単語に分かれる。
fn input() -> TranscriptInput {
    let words = [
        ("これは", (77, 50), (47, 25)),
        ("動画", (47, 25), (111, 50)),
        ("編", (111, 50), (64, 25)),
        ("集", (64, 25), (137, 50)),
        ("ソフト", (137, 50), (163, 50)),
    ];
    TranscriptInput {
        producer: "faster-whisper 1.2.1".into(),
        language: "ja".into(),
        words: words
            .iter()
            .map(|(text, s, e)| WordInput {
                segment: 0,
                start: t(s.0, s.1),
                end: t(e.0, e.1),
                text: (*text).into(),
            })
            .collect(),
    }
}

fn apply(s: &mut Store, op: impl Into<Op>) {
    let txn = Transaction::new("テスト", vec![op.into()]);
    s.apply_txn(txn, &OpContext::human(), actor(), NOW, None)
        .unwrap();
}

fn store_with_words(dir: &std::path::Path) -> Store {
    let mut s = Store::create(dir, &Project::new(), NOW).unwrap();
    apply(
        &mut s,
        AddAsset {
            asset: asset(),
            index: None,
        },
    );
    let n = s.put_transcript(&Id::new("a1"), HASH, &input()).unwrap();
    assert_eq!(n, 5);
    s
}

fn spans(s: &Store, q: &str) -> Vec<(String, RationalTime, RationalTime)> {
    s.search_words(q, 10)
        .unwrap()
        .into_iter()
        .map(|h| (h.word.to_string(), h.start, h.end))
        .collect()
}

fn wid(s: (i128, u32)) -> String {
    word_id(HASH, t(s.0, s.1), 1).to_string()
}

#[test]
fn 単語の境目をまたぐ語も始まりと終わりの時刻で引ける() {
    let tmp = tempfile::tempdir().unwrap();
    let s = store_with_words(tmp.path());
    assert_eq!(
        spans(&s, "編集"),
        [(wid((111, 50)), t(111, 50), t(137, 50))]
    );
    assert_eq!(
        spans(&s, "動画編集ソ"),
        [(wid((47, 25)), t(47, 25), t(163, 50))]
    );
    // 単語の途中から始まる語は、その単語の行で引く。
    assert_eq!(spans(&s, "画編").len(), 1);
    assert!(spans(&s, "存在しない語").is_empty());
    assert!(s.search_words("", 10).is_err());
}

#[test]
fn 消した単語は引けず_直した文字で引け_取り消すと戻る() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = store_with_words(tmp.path());
    let a: AssetId = Id::new("a1");
    apply(
        &mut s,
        DeleteWords {
            asset: a.clone(),
            words: vec![Id::new(wid((47, 25)))],
            restore: false,
        },
    );
    assert!(spans(&s, "動画").is_empty());
    assert_eq!(
        spans(&s, "これは編集").len(),
        1,
        "消した単語をスキップして続く"
    );
    apply(
        &mut s,
        FixTranscript {
            asset: a.clone(),
            word: Id::new(wid((64, 25))),
            text: Some("修".into()),
        },
    );
    assert_eq!(spans(&s, "編修").len(), 1);
    assert!(spans(&s, "編集").is_empty());
    let words = s.transcript_words(&a).unwrap();
    assert_eq!(words[3].shown(), Some("修"));
    assert_eq!(words[1].shown(), None);
    let human = OpContext::human();
    s.undo_last(&human, actor(), NOW).unwrap();
    s.undo_last(&human, actor(), NOW).unwrap();
    assert_eq!(spans(&s, "動画編集").len(), 1);
    drop(s);
    let s = Store::open(tmp.path(), NOW).unwrap();
    assert_eq!(spans(&s, "動画編集").len(), 1, "開き直しても索引が残る");
}

#[test]
fn 外した素材の単語は引かない() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = store_with_words(tmp.path());
    apply(
        &mut s,
        mm_ops::RemoveAsset {
            asset: Id::new("a1"),
        },
    );
    assert!(spans(&s, "編集").is_empty());
    assert!(s.has_transcript(&Id::new("a1")).unwrap());
}

#[test]
fn 話題は行に入り_開き直しと書き出しと取り込みで残る() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = store_with_words(tmp.path());
    let topic = Topic {
        id: Id::new("t1"),
        asset: Id::new("a1"),
        in_point: t(77, 50),
        out_point: t(163, 50),
        title: "挨拶".into(),
        rating: Rating::Favorite,
        tags: vec!["intro".into()],
    };
    apply(&mut s, AddTopic { topic });
    let hits = s.search("挨拶", 10).unwrap();
    assert_eq!(hits[0].kind, "topic");
    s.export_versions().unwrap();
    let p = s.project().clone();
    drop(s);
    let reopened = Store::open(tmp.path(), NOW).unwrap();
    assert_eq!(reopened.project(), &p);
    let other = tempfile::tempdir().unwrap();
    let imported =
        Store::import_versions(other.path(), &tmp.path().join("versions"), false, NOW).unwrap();
    assert_eq!(imported.project().bins, p.bins);
}
