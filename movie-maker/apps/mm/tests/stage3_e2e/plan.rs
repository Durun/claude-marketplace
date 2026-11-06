//! 通しの試験の編集の計画。文字起こしの単語から話題を決め、操作の JSON を作る。

use mm_store::TranscriptWord;
use mm_types::RationalTime;
use serde_json::{json, Value};

/// 話題の数。
pub const TOPICS: usize = 6;
/// 1 つの話題の長さ（秒）の上限。
const TOPIC_SECONDS: f64 = 20.0;
/// 1 つ目の話題を素材の先頭から離す量（秒）。
const TOPIC_MARGIN: f64 = 5.0;
/// 話題に最低限要る単語の数。
const MIN_WORDS: usize = 10;
/// 言い淀みとして探す語。
const FILLERS: [&str; 2] = ["えーと", "あのー"];

/// 話題 1 つ。単語の位置と素材の上の範囲。
#[derive(Debug, Clone)]
pub struct Topic {
    pub id: String,
    pub first: usize,
    pub last: usize,
    pub start: RationalTime,
    pub end: RationalTime,
}

impl Topic {
    pub fn len(&self) -> RationalTime {
        self.end.checked_sub(self.start).unwrap()
    }
}

/// 素材の長さを等分した位置から、`TOPIC_SECONDS` に収まる単語の範囲を話題にする。
pub fn pick_topics(words: &[TranscriptWord], seconds: f64) -> Vec<Topic> {
    (0..TOPICS)
        .map(|k| {
            let from = seconds * k as f64 / TOPICS as f64 + TOPIC_MARGIN;
            let first = words
                .iter()
                .position(|w| w.start.to_f64() >= from)
                .unwrap_or_else(|| panic!("{from} 秒以降の単語が無い"));
            let limit = words[first].start.to_f64() + TOPIC_SECONDS;
            let last = words.iter().rposition(|w| w.end.to_f64() <= limit).unwrap();
            assert!(last >= first + MIN_WORDS, "話題 {k} の単語が少ない");
            Topic {
                id: format!("t{k}"),
                first,
                last,
                start: words[first].start,
                end: words[last].end,
            }
        })
        .collect()
}

/// 話題の中で消す単語の位置。言い淀みの語があればそれ、無ければ中ほどの語。
/// 端の語は、カットの端と重なって割れなくなるので避ける。
pub fn pick_stutter(words: &[TranscriptWord], topic: &Topic) -> (usize, bool) {
    let range = topic.first + 2..topic.last - 2;
    let filler = range.clone().find(|i| {
        let text = words[*i].shown().unwrap_or("");
        FILLERS.iter().any(|f| text.contains(f))
    });
    match filler {
        Some(i) => (i, true),
        None => ((range.start + range.end) / 2, false),
    }
}

pub fn time(t: RationalTime) -> Value {
    serde_json::to_value(t).unwrap()
}

/// トランザクションの JSON（`mm op --txn`）。
pub fn txn(name: &str, ops: Vec<Value>) -> String {
    json!({ "name": name, "ops": ops }).to_string()
}

pub fn add_topic(asset: &str, t: &Topic) -> Value {
    let topic =
        json!({"id": t.id, "asset": asset, "in": time(t.start), "out": time(t.end), "title": t.id});
    json!({"op": "addTopic", "topic": topic})
}

/// 話題の列を末尾に並べる `insertTopics`。カットの ID は話題の ID の `t` を `k` にしたもの。
pub fn insert_topics(sequence: &str, ids: &[&Topic]) -> Value {
    let topics: Vec<&str> = ids.iter().map(|t| t.id.as_str()).collect();
    let cuts: Vec<String> = topics.iter().map(|t| t.replacen('t', "k", 1)).collect();
    json!({"op": "insertTopics", "sequence": sequence, "topics": topics, "cuts": cuts})
}

/// sequence を入れ子にするコンポジション。レイヤーの長さは `length`。
pub fn add_composition(sequence: &str, length: RationalTime) -> Value {
    let source = json!({"kind": "sequence", "version": 1,
        "params": {"sequence": {"value": {"type": "text", "value": sequence}}}});
    let layer = json!({"id": "l1", "source": source, "start": {"num": 0, "den": 1},
        "in": {"num": 0, "den": 1}, "out": time(length),
        "opacity": {"value": {"type": "number", "value": 1.0}}});
    let comp = json!({"id": "main", "width": 640, "height": 360, "fps": {"num": 30, "den": 1},
        "duration": time(length), "layers": [layer]});
    json!({"op": "addComposition", "composition": comp})
}

pub fn add_render_target() -> Value {
    let target = json!({"id": "mp4", "composition": "main", "width": 640, "height": 360,
        "fps": {"num": 30, "den": 1}, "codec": "h264", "output_path": "out.mp4"});
    json!({"op": "addRenderTarget", "target": target})
}

/// 言い淀みの単語を消し、その範囲を sequence `sequence` のカットから抜いて詰める（`cutWords`）。
pub fn delete_stutter(asset: &str, sequence: &str, word: &TranscriptWord) -> Vec<Value> {
    let range = json!({"in": time(word.start), "out": time(word.end)});
    let op = json!({"op": "cutWords", "asset": asset, "sequence": sequence,
        "words": [word.word], "ranges": [range]});
    vec![op]
}
