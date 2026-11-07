use std::fs;

use mm_doc::{Cut, Node, Sequence, WordEdit, SEQUENCE_KIND, SEQUENCE_PARAM};
use mm_types::{ColorSpace, Id, ParamValue};

use super::*;

const HASH: &str = "ca152c0602faf08d897d38f301d5883036d18f9155cac34ae0cc582653c33f75";
const ASSET: &str = "a1";

fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ingest/transcript-ja.json")
}

fn asset() -> Asset {
    Asset {
        id: Id::new(ASSET),
        path: "C:/media/speech-ja.wav".into(),
        content_hash: HASH.into(),
        color_space: ColorSpace::bt709(),
        duration: RationalTime::seconds(5),
        fps: None,
        timebase_offset: RationalTime::zero(),
    }
}

fn secs(n: i64) -> RationalTime {
    RationalTime::seconds(n)
}

/// 付属ファイルだけの素材 1 つのプロジェクト。`cache/ingest/` に日本語の文字起こしのフィクスチャを置く。
fn state(dir: &Path, edit: impl FnOnce(&mut Project)) -> StoryState {
    let cache = dir.join("cache");
    let sidecar = cache.join("ingest").join(format!("{HASH}.transcript.json"));
    fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
    fs::copy(fixture(), sidecar).unwrap();
    let mut project = Project::new();
    project.assets.push(asset());
    edit(&mut project);
    let shared = Arc::new(Shared::new());
    shared.publish_doc(Arc::new(project), Id::new("main"));
    StoryState::new(shared, &dir.join("project.json"), cache)
}

fn word_ids(words: &[StoryWord]) -> Vec<String> {
    words.iter().map(|w| w.id.clone()).collect()
}

fn find<'a>(words: &'a [StoryWord], text: &str) -> &'a StoryWord {
    words.iter().find(|w| w.text == text).unwrap()
}

#[test]
fn 付属ファイルの単語は_db_と同じ_id_になる() {
    let dir = tempfile::tempdir().unwrap();
    let story = state(dir.path(), |_| {});
    let from_sidecar = story.words(ASSET).unwrap();

    let db_dir = dir.path().join("db");
    let mut project = Project::new();
    project.assets.push(asset());
    let mut store = Store::create(&db_dir, &project, 0).unwrap();
    let t = mm_ingest::sidecar::load_transcript(&story.cache, HASH)
        .unwrap()
        .unwrap();
    let input = mm_store::TranscriptInput {
        producer: "test".into(),
        language: t.language.clone(),
        words: t
            .segments
            .iter()
            .flat_map(|s| {
                s.words.iter().map(|w| mm_store::WordInput {
                    segment: 0,
                    start: w.start,
                    end: w.end,
                    text: w.text.clone(),
                })
            })
            .collect(),
    };
    store.put_transcript(&Id::new(ASSET), HASH, &input).unwrap();
    let from_db: Vec<String> = store
        .transcript_words(&Id::new(ASSET))
        .unwrap()
        .iter()
        .map(|w| w.word.to_string())
        .collect();
    assert_eq!(word_ids(&from_sidecar), from_db);
}

#[test]
fn 消した単語と直した文字はいまのプロジェクトのデータから重なる() {
    let dir = tempfile::tempdir().unwrap();
    let probe = state(dir.path(), |_| {});
    let words = probe.words(ASSET).unwrap();
    let (del, fix) = (find(&words, "ソ").id.clone(), find(&words, "編").id.clone());
    let story = state(dir.path(), |p| {
        p.transcript_edits = vec![
            WordEdit {
                asset: Id::new(ASSET),
                word: Id::new(&del),
                text: None,
                deleted: true,
            },
            WordEdit {
                asset: Id::new(ASSET),
                word: Id::new(&fix),
                text: Some("偏".into()),
                deleted: false,
            },
        ];
    });
    let words = story.words(ASSET).unwrap();
    assert!(find(&words, "ソ").deleted);
    assert_eq!(find(&words, "編").fixed.as_deref(), Some("偏"));
    assert!(!find(&words, "フ").deleted);
}

#[test]
fn 範囲は先後どちらから指しても同じ時刻() {
    let dir = tempfile::tempdir().unwrap();
    let story = state(dir.path(), |_| {});
    let words = story.words(ASSET).unwrap();
    let (a, b) = (find(&words, "動画"), find(&words, "ト"));
    let forward = story.range(ASSET, &a.id, &b.id).unwrap();
    assert_eq!(forward, story.range(ASSET, &b.id, &a.id).unwrap());
    assert_eq!((forward.start, forward.end), (a.start, b.end));
    assert!(story.range(ASSET, "w_none", &b.id).is_err());
}

#[test]
fn 検索は単語の境をまたぎ_消した単語を外し_直した文字で探す() {
    let dir = tempfile::tempdir().unwrap();
    let probe = state(dir.path(), |_| {});
    let words = probe.words(ASSET).unwrap();
    let (fix, del) = (find(&words, "編").id.clone(), find(&words, "起").id.clone());
    let story = state(dir.path(), |p| {
        p.transcript_edits = vec![
            WordEdit {
                asset: Id::new(ASSET),
                word: Id::new(&fix),
                text: Some("偏".into()),
                deleted: false,
            },
            WordEdit {
                asset: Id::new(ASSET),
                word: Id::new(&del),
                text: None,
                deleted: true,
            },
        ];
    });
    let hit = &story.search("文字", 10).unwrap()[0];
    assert_eq!(hit.word, find(&words, "文").id);
    assert_eq!(hit.end, find(&words, "字").end);
    assert_eq!(story.search("動画偏集", 10).unwrap().len(), 1);
    assert!(story.search("動画編集", 10).unwrap().is_empty());
    assert_eq!(
        story.search("字こし", 10).unwrap().len(),
        1,
        "消した単語はスキップする"
    );
    assert!(story.search("", 10).is_err());
}

#[test]
fn 検索は_mm_search_と同じ件を返す() {
    let dir = tempfile::tempdir().unwrap();
    let story = state(dir.path(), |_| {});
    let db_dir = dir.path().join("db");
    let mut project = Project::new();
    project.assets.push(asset());
    let mut store = Store::create(&db_dir, &project, 0).unwrap();
    let t = mm_ingest::sidecar::load_transcript(&story.cache, HASH)
        .unwrap()
        .unwrap();
    let words = t.segments[0]
        .words
        .iter()
        .map(|w| mm_store::WordInput {
            segment: 0,
            start: w.start,
            end: w.end,
            text: w.text.clone(),
        })
        .collect();
    let input = mm_store::TranscriptInput {
        producer: "test".into(),
        language: "ja".into(),
        words,
    };
    store.put_transcript(&Id::new(ASSET), HASH, &input).unwrap();
    for q in [
        "の",
        "こんにちは",
        "編集ソフト",
        "文字起こし",
        "です。",
        "試験です",
    ] {
        let want: Vec<_> = store
            .search_words(q, 50)
            .unwrap()
            .iter()
            .map(|h| (h.word.to_string(), h.start, h.end))
            .collect();
        let got: Vec<_> = story
            .search(q, 50)
            .unwrap()
            .iter()
            .map(|h| (h.word.clone(), h.start, h.end))
            .collect();
        assert_eq!(got, want, "検索 {q}");
    }
}

fn sequence_layer(seq: &str) -> mm_doc::Layer {
    let node = Node::new(SEQUENCE_KIND, 1).with_param(SEQUENCE_PARAM, ParamValue::Text(seq.into()));
    mm_doc::Layer::new("ls", node, secs(0), secs(0), secs(60))
}

fn cut(id: &str, from: i64, to: i64) -> Cut {
    Cut::new(id, Node::footage(&Id::new(ASSET)), secs(from), secs(to))
}

#[test]
fn 素材の時刻は_sequence_のカットの位置に変わる() {
    let dir = tempfile::tempdir().unwrap();
    let story = state(dir.path(), |p| {
        p.sequences.push(Sequence {
            id: Id::new("s1"),
            cuts: vec![cut("k1", 10, 14), cut("k2", 30, 33)],
            origin: None,
        });
        p.compositions.push(
            mm_doc::Composition::new(
                Id::new("main"),
                320,
                180,
                mm_types::Rational::integer(30),
                secs(60),
            )
            .with_layers(vec![sequence_layer("s1")]),
        );
    });
    assert_eq!(story.timeline_time(ASSET, 31.0).unwrap(), Some(5.0));
    assert_eq!(story.timeline_time(ASSET, 10.0).unwrap(), Some(0.0));
    assert_eq!(story.timeline_time(ASSET, 20.0).unwrap(), None);
    let overview = story.overview().unwrap();
    assert_eq!(overview.sequences[0].duration, 7.0);
    assert_eq!(overview.open_sequence.as_deref(), Some("s1"));
    assert!(overview.assets[0].has_transcript);
}

#[test]
fn 撮影素材のレイヤーでも素材の時刻が再生位置に変わる() {
    let dir = tempfile::tempdir().unwrap();
    let story = state(dir.path(), |p| {
        let layer = mm_doc::Layer::new(
            "lf",
            Node::footage(&Id::new(ASSET)),
            secs(2),
            secs(10),
            secs(20),
        );
        p.compositions.push(
            mm_doc::Composition::new(
                Id::new("main"),
                320,
                180,
                mm_types::Rational::integer(30),
                secs(30),
            )
            .with_layers(vec![layer]),
        );
    });
    assert_eq!(story.timeline_time(ASSET, 13.0).unwrap(), Some(5.0));
    assert_eq!(story.timeline_time(ASSET, 20.0).unwrap(), None);
    assert_eq!(story.timeline_time("other", 13.0).unwrap(), None);
}

fn at(num: i64, den: u32) -> RationalTime {
    RationalTime::new(i128::from(num), den).unwrap()
}

fn timed(id: &str, start: RationalTime, end: RationalTime) -> StoryWord {
    StoryWord {
        id: id.into(),
        segment: 0,
        start,
        end,
        text: id.into(),
        fixed: None,
        deleted: false,
    }
}

#[test]
fn 範囲の終わりは次の単語の始まりを越えない() {
    let words = [
        timed("a", at(0, 1), at(3, 2)),
        timed("b", at(1, 1), at(2, 1)),
        timed("c", at(2, 1), at(5, 2)),
    ];
    assert_eq!(
        range_end(&words, 0),
        at(1, 1),
        "次の始まりより後に出る終わりは詰める"
    );
    assert_eq!(range_end(&words, 1), at(2, 1), "重ならなければそのまま");
    assert_eq!(range_end(&words, 2), at(5, 2), "最後の単語はそのまま");
}
