import type { Op, Rating, Topic } from "@mm/sdk";
import type { StoryWord, WordRange } from "./api.ts";
import { newId } from "./model.ts";

/**
 * 単語を消し、その範囲の音を `sequence` のカットから抜いて詰める。`range` は選んだ単語の範囲（`story_range`）。
 * 既に表示だけ消した単語も、範囲に入っていれば一緒に抜く。
 */
export const cutOp = (
  asset: string,
  sequence: string,
  selected: readonly StoryWord[],
  range: WordRange,
): Op | null =>
  selected.length === 0
    ? null
    : {
        op: "cutWords",
        asset,
        sequence,
        words: selected.map((w) => w.id),
        ranges: [{ in: range.start, out: range.end }],
      };

/** 単語を表示と検索からだけ消す（カットは変えない）。選んだ単語がみな消してあれば戻す。どちらも対象が無ければ `null`。 */
export const hideOp = (asset: string, selected: readonly StoryWord[]): Op | null => {
  const alive = selected.filter((w) => !w.deleted);
  const restore = alive.length === 0;
  const target = restore ? selected : alive;
  if (target.length === 0) return null;
  return { op: "deleteWords", asset, words: target.map((w) => w.id), restore };
};

/** 1 語の文字を直す。空の文字は直しを外す。 */
export const fixOp = (asset: string, word: StoryWord, text: string): Op => ({
  op: "fixTranscript",
  asset,
  word: word.id,
  text: text === "" || text === word.text ? null : text,
});

/** 範囲に見出しを付けて話題にする。 */
export const addTopicOp = (asset: string, title: string, range: WordRange): Op => ({
  op: "addTopic",
  topic: { id: newId("t"), asset, title, in: range.start, out: range.end },
});

/** 評価を変える。同じ評価をもう一度選ぶと「未評価」に戻す。 */
export const rateOp = (topic: Topic, rating: Rating): Op => ({
  op: "rate",
  target: topic.id,
  rating: topic.rating === rating ? "unrated" : rating,
});

/** タグを付ける・外す。 */
export const tagOp = (topic: Topic, tag: string, remove: boolean): Op => ({
  op: "tag",
  target: topic.id,
  tag,
  remove,
});

/** 話題の列を、並べた順にカットにして sequence の末尾へ差し込む。 */
export const insertTopicsOp = (
  sequence: string,
  topics: readonly Topic[],
): Extract<Op, { op: "insertTopics" }> => ({
  op: "insertTopics",
  sequence,
  topics: topics.map((t) => t.id),
  cuts: topics.map(() => newId("k")),
});
