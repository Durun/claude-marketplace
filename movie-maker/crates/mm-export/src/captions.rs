//! 字幕。コンポジションに置いた sequence のカットの文字起こしを、書き出しの時間軸の字幕にする。
//!
//! 単語は素材の時間軸にあり、カットの `in`・`out`・速さと、レイヤーの置き方を通して書き出しの時間軸に変換する。
//! 消した単語と直した文字は呼ぶ側が [`CaptionWord`] に反映して渡す（DB の `words` とプロジェクトのデータの
//! `transcript_edits` を読むのは操作経路の仕事で、このクレートは DB を知らない）。区切りと折り返しは [`crate::cue_text`]。

use std::collections::BTreeMap;

use mm_decode_api::SubtitleCue;
use mm_doc::{
    Composition, Cut, Project, Sequence, FOOTAGE_KIND, FOOTAGE_PARAM, SEQUENCE_KIND, SEQUENCE_PARAM,
};
use mm_types::{AssetId, RationalTime, Result, TimeRange};

use crate::cue_text::{segment, Word};

/// 1 ms の分母。
const MS_DEN: u32 = 1000;

/// 字幕にする 1 単語。素材の時間軸。
#[derive(Debug, Clone, PartialEq)]
pub struct CaptionWord {
    pub start: RationalTime,
    pub end: RationalTime,
    /// 表示する文字（直した文字を反映済み）。
    pub text: String,
}

/// 素材ごとの、字幕に出す単語（時刻の順。消した単語を含まない）。
pub type TranscriptWords = BTreeMap<AssetId, Vec<CaptionWord>>;

/// ms を時刻にする。
pub fn ms_time(ms: i64) -> RationalTime {
    RationalTime::new(i128::from(ms), MS_DEN).unwrap_or(RationalTime::ZERO)
}

fn to_ms(t: RationalTime) -> i64 {
    (t.to_f64() * 1000.0).round() as i64
}

/// コンポジションの字幕。`range` はその区間だけを、区間の始まりを 0 にして返す。
/// 最上位の有効な sequence のレイヤーだけを見る（入れ子のコンポジションの中の sequence は含めない）。
pub fn composition_cues(
    project: &Project,
    comp: &Composition,
    range: TimeRange,
    transcript: &TranscriptWords,
) -> Result<Vec<SubtitleCue>> {
    let mut words = Vec::new();
    for layer in comp
        .layers
        .iter()
        .filter(|l| l.enabled && l.source.kind == SEQUENCE_KIND)
    {
        let id = layer.source.text_param(SEQUENCE_PARAM).unwrap_or("");
        let seq = project.sequence(&mm_types::Id::new(id))?;
        let window = TimeRange::new(layer.in_point, layer.out_point)?;
        for w in sequence_words(seq, transcript)? {
            let (s, e) = (w.start_ms, w.end_ms);
            let (start, end) = (ms_time(s), ms_time(e));
            if end <= window.start || start >= window.end {
                continue;
            }
            let at = |t: RationalTime| -> Result<i64> {
                let t = t.max(window.start).min(window.end);
                Ok(to_ms(
                    t.checked_sub(layer.in_point)?
                        .div_by(layer.speed)?
                        .checked_add(layer.start)?,
                ))
            };
            words.push(Word {
                start_ms: at(start)?,
                end_ms: at(end)?,
                text: w.text,
            });
        }
    }
    words.sort_by_key(|w| (w.start_ms, w.end_ms));
    let (from, to) = (to_ms(range.start), to_ms(range.end));
    let in_range: Vec<Word> = words
        .into_iter()
        .filter(|w| w.end_ms > from && w.start_ms < to)
        .map(|w| Word {
            start_ms: (w.start_ms - from).max(0),
            end_ms: (w.end_ms.min(to)) - from,
            text: w.text,
        })
        .collect();
    Ok(to_cues(&in_range, to - from))
}

/// sequence をそのまま全長で字幕にする（`mm subtitles`）。時刻は sequence の時間軸。
pub fn sequence_cues(seq: &Sequence, transcript: &TranscriptWords) -> Result<Vec<SubtitleCue>> {
    let length = mm_eval::place_cuts(seq)?
        .iter()
        .map(|p| to_ms(p.range.end))
        .max()
        .unwrap_or(0);
    Ok(to_cues(&sequence_words(seq, transcript)?, length))
}

/// 読む速さで延ばした終わりが書き出しの長さ（`limit_ms`）を超えないよう、長さで切る。
fn to_cues(words: &[Word], limit_ms: i64) -> Vec<SubtitleCue> {
    segment(words)
        .into_iter()
        .map(|c| SubtitleCue {
            start: ms_time(c.start_ms),
            end: ms_time(c.end_ms.min(limit_ms).max(c.start_ms)),
            text: c.text,
        })
        .collect()
}

/// sequence の単語。時刻は sequence の時間軸（ms）。カットの範囲に入った単語だけ。
/// 単語の始まりがカットの中にあれば採り、終わりはカットの終わりで切る（カットをまたぐ単語を二重に出さない）。
fn sequence_words(seq: &Sequence, transcript: &TranscriptWords) -> Result<Vec<Word>> {
    let placed = mm_eval::place_cuts(seq)?;
    let mut out = Vec::new();
    for p in &placed {
        let cut = &seq.cuts[p.index];
        let Some(asset) = footage_asset(cut) else {
            continue;
        };
        let Some(words) = transcript.get(asset) else {
            continue;
        };
        for w in words {
            if w.start < cut.in_point || w.start >= cut.out_point {
                continue;
            }
            let end = w.end.min(cut.out_point);
            let map = |t: RationalTime| -> Result<RationalTime> {
                t.checked_sub(cut.in_point)?
                    .div_by(cut.speed)?
                    .checked_add(p.range.start)
            };
            out.push(Word {
                start_ms: to_ms(map(w.start)?),
                end_ms: to_ms(map(end)?),
                text: w.text.clone(),
            });
        }
    }
    Ok(out)
}

fn footage_asset(cut: &Cut) -> Option<&AssetId> {
    (cut.source.kind == FOOTAGE_KIND)
        .then(|| cut.source.params.get(FOOTAGE_PARAM)?.value.as_asset())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_doc::{Layer, Node};
    use mm_types::{Id, ParamValue, Rational};

    fn t(n: i64, d: u32) -> RationalTime {
        RationalTime::new(i128::from(n), d).unwrap()
    }

    fn word(s: i64, e: i64, text: &str) -> CaptionWord {
        CaptionWord {
            start: t(s, 10),
            end: t(e, 10),
            text: text.into(),
        }
    }

    fn footage(asset: &str) -> Node {
        Node::new(FOOTAGE_KIND, 1).with_param(FOOTAGE_PARAM, ParamValue::AssetRef(Id::new(asset)))
    }

    /// 素材 a の 0〜6 秒の単語「A。」「B。」「C。」「D。」（1 秒おき）。
    fn transcript() -> TranscriptWords {
        let mut m = TranscriptWords::new();
        m.insert(
            Id::new("a"),
            vec![
                word(0, 5, "A。"),
                word(10, 15, "B。"),
                word(20, 25, "C。"),
                word(30, 35, "D。"),
            ],
        );
        m
    }

    fn project(cuts: Vec<Cut>) -> (Project, Composition) {
        let mut p = Project::new();
        p.sequences.push(Sequence {
            id: Id::new("s"),
            cuts,
            origin: None,
        });
        let node =
            Node::new(SEQUENCE_KIND, 1).with_param(SEQUENCE_PARAM, ParamValue::Text("s".into()));
        let layer = Layer::new("l", node, RationalTime::ZERO, RationalTime::ZERO, t(10, 1));
        let comp = Composition::new(Id::new("c"), 16, 9, Rational::integer(30), t(10, 1))
            .with_layers(vec![layer]);
        (p, comp)
    }

    fn whole() -> TimeRange {
        TimeRange::new(RationalTime::ZERO, t(10, 1)).unwrap()
    }

    #[test]
    fn 並べ替えたカットの字幕は_書き出しの時間軸の順に並ぶ() {
        // B（1〜2 秒）を先に、A（0〜1 秒）を後に置く。
        let cuts = vec![
            Cut::new("k1", footage("a"), t(1, 1), t(2, 1)),
            Cut::new("k2", footage("a"), t(0, 1), t(1, 1)),
        ];
        let (p, comp) = project(cuts);
        let cues = composition_cues(&p, &comp, whole(), &transcript()).unwrap();
        let texts: Vec<_> = cues.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["B。", "A。"]);
        assert_eq!(cues[0].start, RationalTime::ZERO);
        assert_eq!(cues[1].start, t(1, 1), "2 件目はカットの長さの分だけ後ろ");
    }

    #[test]
    fn カットの外の単語は入らない() {
        // 1〜3 秒のカットから B と C だけ。D と A は外。
        let (p, comp) = project(vec![Cut::new("k", footage("a"), t(1, 1), t(3, 1))]);
        let cues = composition_cues(&p, &comp, whole(), &transcript()).unwrap();
        let texts: Vec<_> = cues.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["B。", "C。"]);
    }

    #[test]
    fn 速さを変えたカットは単語の時刻も縮む() {
        let mut cut = Cut::new("k", footage("a"), t(0, 1), t(4, 1));
        cut.speed = Rational::integer(2);
        let (p, comp) = project(vec![cut]);
        let cues = composition_cues(&p, &comp, whole(), &transcript()).unwrap();
        // 素材の 2 秒の C は、2 倍速の書き出しの 1 秒。
        assert_eq!(cues[2].start, t(1, 1));
    }

    #[test]
    fn 書き出す区間の外は除き_区間の始まりを_0_にする() {
        let (p, comp) = project(vec![Cut::new("k", footage("a"), t(0, 1), t(4, 1))]);
        let range = TimeRange::new(t(2, 1), t(4, 1)).unwrap();
        let cues = composition_cues(&p, &comp, range, &transcript()).unwrap();
        let texts: Vec<_> = cues.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["C。", "D。"]);
        assert_eq!(cues[0].start, RationalTime::ZERO);
    }

    #[test]
    fn 最後の字幕は読む速さで延ばしても書き出しの長さで切る() {
        let (p, comp) = project(vec![Cut::new("k", footage("a"), t(0, 1), t(1, 2))]);
        let range = TimeRange::new(RationalTime::ZERO, t(1, 2)).unwrap();
        let cues = composition_cues(&p, &comp, range, &transcript()).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].end, t(1, 2));
    }

    #[test]
    fn 無効にしたレイヤーの字幕は出さない() {
        let (p, mut comp) = project(vec![Cut::new("k", footage("a"), t(0, 1), t(4, 1))]);
        comp.layers[0].enabled = false;
        assert!(composition_cues(&p, &comp, whole(), &transcript())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn 文字起こしの無い素材は字幕が空() {
        let (p, comp) = project(vec![Cut::new("k", footage("zzz"), t(0, 1), t(4, 1))]);
        assert!(composition_cues(&p, &comp, whole(), &transcript())
            .unwrap()
            .is_empty());
    }
}
