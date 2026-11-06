//! 長い素材の文字起こしの大きさの確かめ。1 万語の素材で数千語を消す操作の時間を測る。
//! `cargo test -p mm-store --release --test store scale_words -- --ignored --nocapture` で動かす。

use std::time::{Duration, Instant};

use mm_doc::{Asset, Project};
use mm_ops::{AddAsset, DeleteWords, FixTranscript, Op, OpContext, Transaction};
use mm_store::{word_id, Store, TranscriptInput, WordInput};
use mm_types::{ColorSpace, Id, RationalTime};

use crate::common::{actor, NOW};

const WORDS: i128 = 10_000;
const DELETED: i128 = 3_000;
const HASH: &str = "scale-words-hash";
/// 1 回の操作の上限（release）。線形に近い実装なら数十ミリ秒で、2 乗に戻ると数秒になる。
const BUDGET: Duration = Duration::from_millis(1500);

fn word_ids(n: i128) -> Vec<mm_doc::WordId> {
    (0..n)
        .map(|i| word_id(HASH, RationalTime::new(i, 4).unwrap(), 1))
        .collect()
}

fn store() -> (tempfile::TempDir, Store) {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::create(tmp.path(), &Project::new(), NOW).unwrap();
    let asset = Asset {
        id: Id::new("a1"),
        path: "long.wav".into(),
        content_hash: HASH.into(),
        color_space: ColorSpace::bt709(),
        duration: RationalTime::seconds(3600),
        fps: None,
        timebase_offset: RationalTime::ZERO,
    };
    let add = Transaction::new("素材", vec![AddAsset { asset, index: None }.into()]);
    s.apply_txn(add, &OpContext::human(), actor(), NOW, None)
        .unwrap();
    let words = (0..WORDS)
        .map(|i| WordInput {
            segment: (i / 20) as u32,
            start: RationalTime::new(i, 4).unwrap(),
            end: RationalTime::new(i + 1, 4).unwrap(),
            text: format!("語{i}"),
        })
        .collect();
    let input = TranscriptInput {
        producer: "scale".into(),
        language: "ja".into(),
        words,
    };
    s.put_transcript(&Id::new("a1"), HASH, &input).unwrap();
    (tmp, s)
}

fn timed(s: &mut Store, name: &str, op: impl Into<Op>) -> Duration {
    let txn = Transaction::new(name, vec![op.into()]);
    let at = Instant::now();
    s.apply_txn(txn, &OpContext::human(), actor(), NOW, None)
        .unwrap();
    let took = at.elapsed();
    println!("{name}: {took:?}");
    took
}

#[test]
#[ignore = "測定用。リリースビルドで動かす"]
fn 一万語の素材で数千語を消す操作の時間() {
    let (_tmp, mut s) = store();
    let ids = word_ids(WORDS);
    let delete = DeleteWords {
        asset: Id::new("a1"),
        words: ids[..DELETED as usize].to_vec(),
        restore: false,
    };
    let deleted = timed(&mut s, "deleteWords 3000 語", delete.clone());
    let fixed = timed(
        &mut s,
        "fixTranscript 1 語（消した 3000 語の素材）",
        FixTranscript {
            asset: Id::new("a1"),
            word: ids[5000].clone(),
            text: Some("直した".into()),
        },
    );
    let restored = timed(
        &mut s,
        "deleteWords 3000 語を戻す",
        DeleteWords {
            restore: true,
            ..delete
        },
    );
    assert!(
        deleted < BUDGET && fixed < BUDGET && restored < BUDGET,
        "{deleted:?} {fixed:?} {restored:?}"
    );
}
