import type { RationalTime, Topic } from "@mm/sdk";
import type { StoryWord } from "./api.ts";

const SECONDS_PER_MINUTE = 60;
const ID_RADIX = 36;

/** 表示する文字。直した文字があればそれ。 */
export const shownText = (w: StoryWord): string => w.fixed ?? w.text;

/** 時刻を秒に。 */
export const toSeconds = (t: RationalTime): number => t.num / t.den;

/** 2 つの時刻の大小。分数のまま比べる。 */
export const compareTime = (a: RationalTime, b: RationalTime): number =>
  Math.sign(a.num * b.den - b.num * a.den);

/** `m:ss.t` の形。 */
export const formatClock = (seconds: number): string => {
  const total = Math.max(0, seconds);
  const minutes = Math.floor(total / SECONDS_PER_MINUTE);
  const rest = (total - minutes * SECONDS_PER_MINUTE).toFixed(1).padStart("00.0".length, "0");
  return `${minutes}:${rest}`;
};

let counter = 0;

/** 操作に渡す ID。呼び出し側が決める（`insertTopics` のカットや `addTopic` の話題）。 */
export const newId = (prefix: string): string => {
  counter += 1;
  return `${prefix}${Date.now().toString(ID_RADIX)}${counter.toString(ID_RADIX)}`;
};

/** 連続した単語の塊。同じ話題に入る単語を段落（区間）ごとに分ける。`topic` が `null` は話題に入らない単語。 */
export interface Block {
  key: string;
  topic: Topic | null;
  /** 見出しを出すか。話題ごとに最初の塊だけ（重なった話題で塊が分かれても見出しは 1 回）。 */
  head: boolean;
  /** 段落ごとの単語と、その単語の `words` の中の番号。 */
  paragraphs: { word: StoryWord; index: number }[][];
}

/** 単語が入る話題。範囲 `[in, out)` に始まりが入る話題のうち、始まりが一番遅いもの（重なった話題は、後から始まる方の話題に入れる）。 */
const topicOf = (word: StoryWord, topics: readonly Topic[]): Topic | null =>
  topics.findLast((t) => compareTime(t.in, word.start) <= 0 && compareTime(word.start, t.out) < 0) ??
  null;

/** 単語の列を、話題ごとの塊に分ける。話題は素材の時刻の順に並べる。 */
export const buildBlocks = (words: readonly StoryWord[], topics: readonly Topic[]): Block[] => {
  const sorted = [...topics].sort((a, b) => compareTime(a.in, b.in));
  const blocks: Block[] = [];
  const headed = new Set<string>();
  let lastSegment = -1;
  words.forEach((word, index) => {
    const topic = topicOf(word, sorted);
    let block = blocks.at(-1);
    if (!block || block.topic?.id !== topic?.id) {
      const head = topic !== null && !headed.has(topic.id);
      if (topic) headed.add(topic.id);
      block = { key: `${topic?.id ?? "plain"}-${index}`, topic, head, paragraphs: [] };
      blocks.push(block);
      lastSegment = -1;
    }
    if (word.segment !== lastSegment) block.paragraphs.push([]);
    lastSegment = word.segment;
    block.paragraphs.at(-1)?.push({ word, index });
  });
  return blocks;
};

/** 単語の始まりを 1 つも含まず、本文のどの塊にも出ない話題。見出しを末尾に一覧で出して外せるようにする。 */
export const orphanTopics = (blocks: readonly Block[], topics: readonly Topic[]): Topic[] => {
  const shown = new Set(blocks.flatMap((b) => (b.topic ? [b.topic.id] : [])));
  return topics.filter((t) => !shown.has(t.id));
};

/** 選んだ単語の範囲（`words` の番号、両端を含む）。 */
export interface Span {
  from: number;
  to: number;
}

/** つかんだ位置と今の位置から、小さい方を先にした範囲を作る。 */
export const spanOf = (anchor: number, focus: number): Span => ({
  from: Math.min(anchor, focus),
  to: Math.max(anchor, focus),
});

/** 選んだ範囲の単語。 */
export const wordsIn = (words: readonly StoryWord[], span: Span | null): StoryWord[] =>
  span ? words.slice(span.from, span.to + 1) : [];

/** 話題の見出しの既定。選んだ単語の文字を先頭から。 */
export const defaultTitle = (selected: readonly StoryWord[], maxChars: number): string =>
  selected
    .filter((w) => !w.deleted)
    .map(shownText)
    .join("")
    .slice(0, maxChars);
