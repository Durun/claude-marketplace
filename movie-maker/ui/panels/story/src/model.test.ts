import type { Topic } from "@mm/sdk";
import type { StoryWord } from "./api.ts";
import { describe, expect, it } from "vitest";
import { buildBlocks, compareTime, defaultTitle, formatClock, newId, orphanTopics, spanOf } from "./model.ts";

const word = (id: string, start: number, segment = 0, over: Partial<StoryWord> = {}): StoryWord => ({
  id,
  segment,
  start: { num: start, den: 2 },
  end: { num: start + 1, den: 2 },
  text: id,
  fixed: null,
  deleted: false,
  ...over,
});

const topic = (id: string, from: number, to: number): Topic => ({
  id,
  asset: "a1",
  title: id,
  in: { num: from, den: 2 },
  out: { num: to, den: 2 },
});

describe("buildBlocks", () => {
  const words = [word("a", 0), word("b", 1), word("c", 2, 1), word("d", 3, 1), word("e", 4, 1)];

  it("話題ごとに塊を分け、話題に入らない単語は見出しの無い塊になる", () => {
    const blocks = buildBlocks(words, [topic("t1", 1, 3)]);
    expect(blocks.map((b) => [b.topic?.id ?? null, b.paragraphs.flat().map((p) => p.word.id)])).toEqual([
      [null, ["a"]],
      ["t1", ["b", "c"]],
      [null, ["d", "e"]],
    ]);
  });

  it("区間が変わるところで段落を分け、単語の番号を持つ", () => {
    const [block] = buildBlocks(words, []);
    expect(block?.paragraphs.map((p) => p.map((x) => x.index))).toEqual([
      [0, 1],
      [2, 3, 4],
    ]);
  });

  it("話題の範囲は始まりを含み終わりを含まない", () => {
    const blocks = buildBlocks(words, [topic("t1", 0, 2)]);
    expect(blocks[0]?.paragraphs.flat().map((p) => p.word.id)).toEqual(["a", "b"]);
  });

  it("話題が重なったところは、後から始まる話題に入れる", () => {
    const blocks = buildBlocks(words, [topic("t1", 0, 4), topic("t2", 2, 6)]);
    expect(blocks.map((b) => [b.topic?.id, b.paragraphs.flat().map((p) => p.word.id)])).toEqual([
      ["t1", ["a", "b"]],
      ["t2", ["c", "d", "e"]],
    ]);
  });

});

describe("buildBlocks の見出し", () => {
  const words = [word("a", 0), word("b", 1), word("c", 2, 1), word("d", 3, 1), word("e", 4, 1)];

  it("見出しは話題ごとに 1 回で、重なって塊が分かれた話題の 2 つ目には出ない", () => {
    const more = [...words, word("f", 5, 1), word("g", 6, 1)];
    // t1 は a から g、t2 は c から d。t1 は t2 をまたいで 2 つの塊になる。
    const blocks = buildBlocks(more, [topic("t1", 0, 8), topic("t2", 2, 4)]);
    expect(blocks.map((b) => [b.topic?.id, b.head])).toEqual([
      ["t1", true],
      ["t2", true],
      ["t1", false],
    ]);
  });

  it("単語の始まりを含まない話題は、本文に出ず末尾の一覧に回る", () => {
    const topics = [topic("t1", 0, 2), topic("far", 100, 120)];
    expect(orphanTopics(buildBlocks(words, topics), topics).map((t) => t.id)).toEqual(["far"]);
  });

});

describe("buildBlocks の順序", () => {
  const words = [word("a", 0), word("b", 1), word("c", 2, 1), word("d", 3, 1), word("e", 4, 1)];

  it("話題は渡す順に関わらず時刻の順に並べる", () => {
    const blocks = buildBlocks(words, [topic("late", 3, 5), topic("early", 0, 2)]);
    expect(blocks.map((b) => b.topic?.id ?? null)).toEqual(["early", null, "late"]);
  });
});

describe("時刻と選択", () => {
  it("分数のまま大小を比べる", () => {
    expect(compareTime({ num: 1, den: 3 }, { num: 2, den: 6 })).toBe(0);
    expect(compareTime({ num: 1, den: 3 }, { num: 1, den: 2 })).toBe(-1);
  });

  it("m:ss.t の形にする", () => {
    expect(formatClock(0)).toBe("0:00.0");
    expect(formatClock(75.25)).toBe("1:15.3");
  });

  it("範囲は先後どちらからつかんでも小さい方が先", () => {
    expect(spanOf(5, 2)).toEqual({ from: 2, to: 5 });
  });

  it("見出しの既定は消していない単語の文字を先頭から", () => {
    const selected = [word("あ", 0), word("い", 1, 0, { deleted: true }), word("う", 2, 0, { fixed: "宇" })];
    expect(defaultTitle(selected, 5)).toBe("あ宇");
    expect(defaultTitle(selected, 1)).toBe("あ");
  });

  it("ID は呼ぶたびに変わる", () => {
    expect(newId("k")).not.toBe(newId("k"));
  });
});
