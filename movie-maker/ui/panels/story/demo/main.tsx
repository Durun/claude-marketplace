// Story パネルをモックの API で動かすページ。スクリーンショット用で、アプリには載せない。
// ?state=plain|topics で最初の状態を選ぶ。編集の操作は手元の配列に反映する。
import { render } from "preact";
import type { Op, RationalTime, Topic } from "@mm/sdk";
import type { StoryApi, StoryTranscript, StoryWord } from "../src/api.ts";
import { followTheme } from "../../../shell/src/followTheme.ts";
import { StoryPanel } from "../src/index.ts";
import "../../../shell/src/shell.css";

const SCRIPT = [
  "えー|今日は|動画|編集|ソフトの|使い方|を|紹介|します|。",
  "まず|素材|を|取り込み|ます|。",
  "あの|取り込み|は|ドラッグ|でも|できます|。",
  "次に|文字|起こし|を|見ます|。",
  "ここ|で|話題|ごとに|区切っ|て|並べ|られ|ます|。",
  "ちょっと|脱線|ですが|、|昨日|の|ランチ|は|ラーメン|でした|。",
  "さて|本題|に|戻り|ます|。",
  "不要|な|部分|は|削除|できます|。",
  "書き出し|は|ワンクリック|です|。",
  "以上|です|。|ありがとう|ござい|ました|。",
];
const CENTI = 100;
const SECONDS_PER_CHAR = 0.16;
const GAP_SECONDS = 0.08;

const time = (seconds: number): RationalTime => ({ num: Math.round(seconds * CENTI), den: CENTI });
const seconds = (t: RationalTime): number => t.num / t.den;

const buildWords = (): StoryWord[] => {
  let head = 0;
  return SCRIPT.flatMap((line, segment) =>
    line.split("|").map((text, i): StoryWord => {
      const start = head;
      head += text.length * SECONDS_PER_CHAR + GAP_SECONDS;
      return { id: `w${segment}_${i}`, segment, start: time(start), end: time(head), text, fixed: null, deleted: false };
    }),
  );
};

const words = buildWords();
const at = (id: string): StoryWord => words.find((w) => w.id === id) as StoryWord;
const scene = new URLSearchParams(location.search).get("state") ?? "plain";

const topics: Topic[] =
  scene === "topics"
    ? [
        { id: "t1", asset: "a1", title: "あいさつ", in: at("w0_0").start, out: at("w0_9").end, rating: "favorite", tags: ["intro"] },
        { id: "t2", asset: "a1", title: "素材の取り込み", in: at("w1_0").start, out: at("w2_6").end, tags: ["tutorial"] },
        { id: "t3", asset: "a1", title: "脱線 (ランチ)", in: at("w5_0").start, out: at("w5_10").end, rating: "rejected" },
        { id: "t4", asset: "a1", title: "書き出しとおわり", in: at("w8_0").start, out: at("w9_6").end, rating: "favorite" },
      ]
    : [];
if (scene === "topics") {
  at("w0_0").deleted = true;
  at("w2_0").fixed = "あの、";
}

let cuts = 0;
const transcript = (): StoryTranscript => ({
  asset: "a1",
  words: words.map((w) => ({ ...w })),
  topics: topics.map((t) => ({ ...t })),
  selections: [],
});

const applyOp = (op: Op): void => {
  if (op.op === "addTopic") topics.push(op.topic);
  if (op.op === "removeTopic") topics.splice(topics.findIndex((t) => t.id === op.topic), 1);
  if (op.op === "deleteWords") op.words.forEach((id) => (at(id).deleted = !op.restore));
  if (op.op === "cutWords") (op.words ?? []).forEach((id) => (at(id).deleted = true));
  if (op.op === "fixTranscript") at(op.word).fixed = op.text ?? null;
  if (op.op === "rate") (topics.find((t) => t.id === op.target) as Topic).rating = op.rating;
  if (op.op === "tag") {
    const target = topics.find((t) => t.id === op.target) as Topic;
    const rest = (target.tags ?? []).filter((x) => x !== op.tag);
    target.tags = (op.remove ? rest : [...rest, op.tag]).sort();
  }
  if (op.op === "insertTopics") cuts += op.topics.length;
};

const api: StoryApi = {
  overview: async () => ({
    assets: [{ id: "a1", name: "tutorial-ja.mp4", duration: 42, has_transcript: true }],
    sequences: [{ id: "main-cut", cuts, duration: cuts * 5 }],
  }),
  transcript: async () => transcript(),
  range: async (_asset, first, last) => {
    const [a, b] = [words.findIndex((w) => w.id === first), words.findIndex((w) => w.id === last)];
    return { start: (words[Math.min(a, b)] as StoryWord).start, end: (words[Math.max(a, b)] as StoryWord).end };
  },
  search: async (query) =>
    words
      .filter((w) => w.text.includes(query))
      .map((w) => ({ asset: "a1", word: w.id, start: w.start, end: w.end, context: `${w.text}…` })),
  timelineTime: async (_asset, secondsAt) => secondsAt,
  cutTime: async () => 0,
  apply: async (op) => {
    applyOp(op);
    return { ok: true, value: {} };
  },
  seek: async (s) => console.debug("seek", s, seconds(time(s))),
};

followTheme(document.documentElement, location.search, matchMedia.bind(window));
const root = document.getElementById("root");
if (root) {
  root.style.boxSizing = "border-box";
  root.style.flexDirection = "column";
  root.style.padding = "10px";
  root.style.width = "260px";
  root.style.height = "100vh";
  render(<StoryPanel api={api} />, root);
}
