// @vitest-environment jsdom
import { render } from "preact";
import { act } from "preact/test-utils";
import type { Op, Topic } from "@mm/sdk";
import type { StoryApi, StoryTranscript, StoryWord } from "./api.ts";
import { afterEach, describe, expect, it, vi } from "vitest";
import { StoryPanel } from "./StoryPanel.tsx";

const host = document.createElement("div");
document.body.append(host);
afterEach(() => render(null, host));

const t = (n: number) => ({ num: n, den: 2 });
const word = (id: string, n: number, over: Partial<StoryWord> = {}): StoryWord => ({
  id,
  segment: 0,
  start: t(n),
  end: t(n + 1),
  text: id,
  fixed: null,
  deleted: false,
  ...over,
});
const topic = (id: string, from: number, to: number, over: Partial<Topic> = {}): Topic => ({
  id,
  asset: "a1",
  title: `話題${id}`,
  in: t(from),
  out: t(to),
  ...over,
});

const transcript = (topics: Topic[] = []): StoryTranscript => ({
  asset: "a1",
  words: [
    word("w0", 0),
    word("w1", 1),
    word("w2", 2, { deleted: true }),
    word("w3", 3, { fixed: "直" }),
    word("w4", 4),
  ],
  topics,
  selections: [],
});

const settle = () => act(() => new Promise<void>((ok) => setTimeout(ok, 0)));

const fake = (topics: Topic[] = [], over: Partial<StoryApi> = {}): StoryApi => ({
  overview: vi.fn().mockResolvedValue({
    assets: [{ id: "a1", name: "talk.wav", duration: 5, has_transcript: true }],
    sequences: [{ id: "s1", cuts: 0, duration: 0 }],
  }),
  transcript: vi.fn().mockResolvedValue(transcript(topics)),
  range: vi.fn().mockResolvedValue({ start: t(1), end: t(4) }),
  search: vi.fn().mockResolvedValue([]),
  timelineTime: vi.fn().mockResolvedValue(2.5),
  cutTime: vi.fn().mockResolvedValue(7.5),
  apply: vi.fn().mockResolvedValue({ ok: true, value: {} }),
  seek: vi.fn().mockResolvedValue(undefined),
  ...over,
});

const mount = async (api: StoryApi) => {
  act(() => render(<StoryPanel api={api} />, host));
  await settle();
};

const wordEl = (i: number) => host.querySelector(`[data-i="${i}"]`) as HTMLElement;
const mouse = (el: EventTarget, type: string, init: MouseEventInit = {}) =>
  act(() => void el.dispatchEvent(new MouseEvent(type, { bubbles: true, button: 0, ...init })));
const drag = (from: number, to: number) => {
  mouse(wordEl(from), "mousedown");
  mouse(wordEl(to), "mouseover");
  mouse(window, "mouseup");
};
const button = (label: string) =>
  [...host.querySelectorAll("button")].find((b) => b.textContent?.trim() === label) as HTMLElement;
const press = async (el: HTMLElement) => {
  act(() => el.click());
  await settle();
};
const sent = (api: StoryApi): Op[] => vi.mocked(api.apply).mock.calls.map(([op]) => op);

describe("StoryPanel の表示", () => {
  it("消した単語と直した単語を区別して出し、話題ごとに区切る", async () => {
    await mount(fake([topic("t1", 1, 3)]));
    expect(wordEl(2).dataset.deleted).toBe("true");
    expect(wordEl(3).dataset.fixed).toBe("true");
    expect(wordEl(3).textContent).toBe("直");
    expect(host.querySelectorAll(".mm-story-block").length).toBe(3);
    expect(host.querySelector(".mm-story-topic-title")?.textContent).toBe("話題t1");
  });

  it("文字起こしの無いときは案内を出す", async () => {
    const api = fake([], {
      overview: vi.fn().mockResolvedValue({ assets: [], sequences: [] }),
    });
    await mount(api);
    expect(host.querySelector(".mm-story-empty")).not.toBeNull();
  });
});

describe("StoryPanel のキーボード", () => {
  const key = (el: HTMLElement, k: string, init: KeyboardEventInit = {}) => {
    const e = new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...init });
    act(() => void el.dispatchEvent(e));
    return e;
  };

  it("Tab で止まる単語は 1 つだけで、矢印で移る", async () => {
    await mount(fake());
    const stops = () => [...host.querySelectorAll(".mm-story-word")].filter((w) => w.getAttribute("tabindex") === "0");
    expect(stops().length).toBe(1);
    key(wordEl(0), "ArrowRight");
    expect(stops()[0]).toBe(wordEl(1));
    expect(document.activeElement === wordEl(1) || host.contains(document.activeElement)).toBe(true);
  });

  it("Shift+矢印で範囲を伸ばし、処理したキーは外へ伝えない", async () => {
    await mount(fake());
    const outside = vi.fn();
    window.addEventListener("keydown", outside);
    key(wordEl(1), "ArrowRight", { shiftKey: true });
    key(wordEl(2), "ArrowRight", { shiftKey: true });
    window.removeEventListener("keydown", outside);
    expect([1, 2, 3].map((i) => wordEl(i).dataset.selected)).toEqual(["true", "true", "true"]);
    expect(wordEl(0).dataset.selected).toBe("false");
    expect(outside).not.toHaveBeenCalled();
  });

  it("Enter で 1 語を選び、Escape で外す", async () => {
    await mount(fake());
    key(wordEl(2), "Enter");
    expect(wordEl(2).dataset.selected).toBe("true");
    key(wordEl(2), "Escape");
    expect(wordEl(2).dataset.selected).toBe("false");
  });
});

describe("StoryPanel の素材の選択", () => {
  it("素材を選ぶと Source で開くためのイベントを投げる", async () => {
    const api = fake([], {
      overview: vi.fn().mockResolvedValue({
        assets: [
          { id: "a1", name: "a.wav", duration: 5, has_transcript: true },
          { id: "a2", name: "b.wav", duration: 5, has_transcript: true },
        ],
        sequences: [],
      }),
    });
    await mount(api);
    const seen = vi.fn();
    window.addEventListener("mm:asset-selected", (e) => seen((e as CustomEvent).detail));
    const select = host.querySelector<HTMLSelectElement>("select[aria-label=Clip]");
    act(() => {
      if (select) select.value = "a2";
      select?.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(seen).toHaveBeenCalledWith({ asset: "a2" });
  });
});

describe("StoryPanel の選択と編集", () => {
  it("ドラッグで選んだ範囲の時刻を聞いて話題にする", async () => {
    const api = fake();
    await mount(api);
    drag(1, 3);
    expect(host.querySelectorAll('[data-selected="true"]').length).toBe(3);
    await press(button("Make topic"));
    expect(api.range).toHaveBeenCalledWith("a1", "w1", "w3");
    const [op] = sent(api);
    expect(op).toMatchObject({ op: "addTopic", topic: { asset: "a1", in: t(1), out: t(4), title: "w1直" } });
    expect(api.transcript).toHaveBeenCalledTimes(2);
  });
});

describe("StoryPanel の Delete", () => {
  it("Delete は選んだ単語の範囲を並べ先の sequence から外す cutWords を送る", async () => {
    const api = fake();
    await mount(api);
    drag(0, 2);
    await press(button("Delete"));
    expect(api.range).toHaveBeenCalledWith("a1", "w0", "w2");
    expect(sent(api)[0]).toEqual({
      op: "cutWords",
      asset: "a1",
      sequence: "s1",
      words: ["w0", "w1", "w2"],
      ranges: [{ in: t(1), out: t(4) }],
    });
  });
});

describe("StoryPanel の Delete の切る先、Hide、修正", () => {
  it("開いているコンポジションの sequence があればそこを切り、sequence が無ければ送らない", async () => {
    const sequences = [
      { id: "s1", cuts: 0, duration: 0 },
      { id: "short", cuts: 0, duration: 0 },
    ];
    const assets = [{ id: "a1", name: "talk.wav", duration: 5, has_transcript: true }];
    const open = fake([], { overview: vi.fn().mockResolvedValue({ assets, sequences, open_sequence: "short" }) });
    await mount(open);
    drag(0, 0);
    await press(button("Delete"));
    expect(sent(open)[0]).toMatchObject({ op: "cutWords", sequence: "short" });
    const none = fake([], { overview: vi.fn().mockResolvedValue({ assets, sequences: [] }) });
    await mount(none);
    drag(0, 0);
    await press(button("Delete"));
    expect(sent(none)).toEqual([]);
  });

  it("Hide は消していない単語を表示だけ消し、全部消してあれば戻す", async () => {
    const api = fake();
    await mount(api);
    drag(0, 2);
    await press(button("Hide"));
    expect(sent(api)[0]).toEqual({ op: "deleteWords", asset: "a1", words: ["w0", "w1"], restore: false });
    drag(2, 2);
    expect(button("Restore")).toBeDefined();
    await press(button("Restore"));
    expect(sent(api)[1]).toEqual({ op: "deleteWords", asset: "a1", words: ["w2"], restore: true });
  });

  it("1 語を選ぶと直す欄が出て、修正を送る", async () => {
    const api = fake();
    await mount(api);
    drag(1, 1);
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Fixed text"]');
    act(() => {
      if (input) input.value = "壱";
      input?.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await press(button("Fix"));
    expect(sent(api)[0]).toEqual({ op: "fixTranscript", asset: "a1", word: "w1", text: "壱" });
  });

});

describe("StoryPanel の失敗と再生位置", () => {
  it("断られた理由を出す", async () => {
    const api = fake([], { apply: vi.fn().mockResolvedValue({ ok: false, error: "既に消してある" }) });
    await mount(api);
    drag(0, 0);
    await press(button("Delete"));
    expect(host.querySelector(".mm-story-error")?.textContent).toBe("既に消してある");
  });

  it("単語をクリックすると再生位置がその時刻に移る", async () => {
    const api = fake();
    await mount(api);
    drag(3, 3);
    await settle();
    expect(api.timelineTime).toHaveBeenCalledWith("a1", 1.5);
    expect(api.seek).toHaveBeenCalledWith(2.5);
  });

  it("シーケンスに入っていない単語は動かさず知らせる", async () => {
    const api = fake([], { timelineTime: vi.fn().mockResolvedValue(null) });
    await mount(api);
    drag(0, 0);
    await settle();
    expect(api.seek).not.toHaveBeenCalled();
    expect(host.querySelector(".mm-story-meta")?.textContent).toContain("not on the timeline");
  });
});

describe("StoryPanel の評価とタグ", () => {
  it("評価を押す。同じ評価をもう一度押すと未評価に戻る", async () => {
    const api = fake([topic("t1", 0, 2, { rating: "favorite" })]);
    await mount(api);
    await press(button("Skip"));
    await press(button("Use"));
    expect(sent(api)).toEqual([
      { op: "rate", target: "t1", rating: "rejected" },
      { op: "rate", target: "t1", rating: "unrated" },
    ]);
  });

  it("タグを付け、押して外す", async () => {
    const api = fake([topic("t1", 0, 2, { tags: ["intro"] })]);
    await mount(api);
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Add tag"]');
    act(() => {
      if (input) input.value = "demo";
      input?.dispatchEvent(new Event("input", { bubbles: true }));
    });
    act(() => void input?.form?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })));
    await settle();
    await press(button("intro"));
    expect(sent(api)).toEqual([
      { op: "tag", target: "t1", tag: "demo", remove: false },
      { op: "tag", target: "t1", tag: "intro", remove: true },
    ]);
  });
});

describe("StoryPanel の検索と並べる", () => {
  it("検索の結果を押すと、その単語へ移る", async () => {
    const hit = { asset: "a1", word: "w3", start: t(3), end: t(4), context: "直の続き" };
    const api = fake([], { search: vi.fn().mockResolvedValue([hit]) });
    await mount(api);
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Search transcript"]');
    act(() => {
      if (input) input.value = "直";
      input?.dispatchEvent(new Event("input", { bubbles: true }));
    });
    act(() => void input?.form?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })));
    await settle();
    expect(api.search).toHaveBeenCalledWith("直");
    await press(host.querySelector(".mm-story-hit") as HTMLElement);
    expect(wordEl(3).dataset.focus).toBe("true");
    expect(api.seek).toHaveBeenCalledWith(2.5);
  });

  it("選んだ話題を並べた順に sequence へ差し込み、先頭へ移る", async () => {
    const api = fake([topic("t1", 0, 1), topic("t2", 3, 5)]);
    await mount(api);
    const picks = host.querySelectorAll<HTMLInputElement>('input[aria-label="Pick topic"]');
    await press(picks[1] as HTMLElement);
    await press(picks[0] as HTMLElement);
    expect(host.querySelectorAll(".mm-story-tray-item").length).toBe(2);
    await press(button("Arrange"));
    const [op] = sent(api);
    expect(op).toMatchObject({ op: "insertTopics", sequence: "s1", topics: ["t2", "t1"] });
    expect((op as { cuts: string[] }).cuts.length).toBe(2);
    const [firstCut] = (op as { cuts: string[] }).cuts;
    expect(api.cutTime).toHaveBeenCalledWith(firstCut);
    expect(api.seek).toHaveBeenCalledWith(7.5);
    expect(host.querySelector(".mm-story-tray")).toBeNull();
  });
});
