// @vitest-environment jsdom
import { render } from "preact";
import { act } from "preact/test-utils";
import type { Layer } from "@mm/sdk";
import type { DeliverApi } from "../../panels/deliver/src/api.ts";
import type { MediaApi } from "../../panels/media/src/api.ts";
import type { MixerApi } from "../../panels/mixer/src/api.ts";
import type { TransitionApi } from "../../panels/transition/src/api.ts";
import type { ReviewApi } from "../../panels/review/src/api.ts";
import type { StoryApi } from "../../panels/story/src/api.ts";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LAYER_SELECTED_EVENT, Shell } from "./Shell.tsx";

const host = document.createElement("div");
document.body.append(host);
afterEach(() => render(null, host));

const t0 = { num: 0, den: 1 };
const layer = {
  id: "l1",
  start: t0,
  in: t0,
  out: t0,
  opacity: { value: { type: "number", value: 1 } },
  source: { kind: "solid", version: 1 },
} as Layer;

const fakeMedia = (over: Partial<MediaApi> = {}): MediaApi => ({
  list: vi.fn().mockResolvedValue([]),
  thumbnail: vi.fn().mockResolvedValue(new ArrayBuffer(0)),
  waveform: vi.fn().mockResolvedValue({ duration: 0, peaks: [] }),
  pickFiles: vi.fn().mockResolvedValue([]),
  importFiles: vi.fn().mockResolvedValue(undefined),
  cacheInfo: vi.fn().mockResolvedValue({ total_bytes: 0, folders: [] }),
  clearCache: vi.fn().mockResolvedValue(0),
  onProgress: vi.fn().mockResolvedValue(() => {}),
  onDrop: vi.fn().mockResolvedValue(() => {}),
  ...over,
});

const fakeStory = (): StoryApi => ({
  overview: vi.fn().mockResolvedValue({ assets: [], sequences: [] }),
  transcript: vi.fn().mockResolvedValue({ asset: "", words: [], topics: [], selections: [] }),
  range: vi.fn(),
  search: vi.fn().mockResolvedValue([]),
  timelineTime: vi.fn().mockResolvedValue(null),
  cutTime: vi.fn().mockResolvedValue(null),
  apply: vi.fn().mockResolvedValue({ ok: true, value: {} }),
  seek: vi.fn(),
});

const fakeDeliver = (): DeliverApi => ({
  overview: vi.fn().mockResolvedValue({ targets: [], compositions: [], sequences: [], open_composition: null, can_verify: false }),
  status: vi.fn().mockResolvedValue({ running: false, items: [] }),
  applyOps: vi.fn().mockResolvedValue(undefined),
  start: vi.fn().mockResolvedValue(undefined),
  cancel: vi.fn().mockResolvedValue(undefined),
  reveal: vi.fn().mockResolvedValue(undefined),
  pickOutput: vi.fn().mockResolvedValue(null),
  onProgress: vi.fn().mockResolvedValue(() => {}),
});

const fakeMixer = (): MixerApi => ({
  state: vi.fn().mockResolvedValue({ composition: "main", mixer: null, tracks: [], roles: ["voice", "bgm", "sfx"] }),
  applyOps: vi.fn().mockResolvedValue(undefined),
  onMeters: vi.fn().mockResolvedValue(() => {}),
});

const fakeReview = (): ReviewApi => ({
  list: vi.fn().mockResolvedValue([]),
  approve: vi.fn(),
  reject: vi.fn(),
  reopen: vi.fn(),
  undo: vi.fn(),
  preview: vi.fn().mockResolvedValue(undefined),
  onChanged: vi.fn().mockResolvedValue(() => {}),
});

const click = (el: Element | null | undefined) =>
  act(() => {
    (el as HTMLElement).click();
  });

describe("Shell の左の列", () => {
  it("Media を渡さないときはタブを出さず Tasks だけ", () => {
    act(() => render(<Shell sendOp={vi.fn()} />, host));
    expect(host.querySelector(".mm-shell-tabs")).toBeNull();
    expect(host.querySelector(".mm-tasks")).not.toBeNull();
  });

  it("タブで Tasks と Media を切り替え、どちらも作ったまま残す", () => {
    act(() => render(<Shell sendOp={vi.fn()} apis={{ media: fakeMedia() }} />, host));
    const panels = host.querySelectorAll<HTMLElement>(".mm-shell-left-body");
    expect([panels[0]?.hidden, panels[1]?.hidden]).toEqual([false, true]);
    click(host.querySelectorAll(".mm-shell-tab")[1]);
    expect([panels[0]?.hidden, panels[1]?.hidden]).toEqual([true, false]);
    expect(host.querySelector(".mm-media")).not.toBeNull();
  });

  it("Story を渡すと Story のタブが出て、開くたびに読み直す", async () => {
    const story = fakeStory();
    act(() => render(<Shell sendOp={vi.fn()} apis={{ media: fakeMedia(), story }} />, host));
    const tabs = host.querySelectorAll(".mm-shell-tab");
    expect([...tabs].map((t) => t.textContent)).toEqual(["Tasks", "Media", "Story"]);
    const panels = host.querySelectorAll<HTMLElement>(".mm-shell-left-body");
    expect(panels[2]?.hidden).toBe(true);
    click(tabs[2]);
    await act(() => new Promise<void>((ok) => setTimeout(ok, 0)));
    expect(panels[2]?.hidden).toBe(false);
    expect(host.querySelector(".mm-story")).not.toBeNull();
    expect(story.overview).toHaveBeenCalledTimes(2);
  });

  it("Deliver を渡すとタブが出て、Tasks の書き出しの項目から開ける", async () => {
    act(() => render(<Shell sendOp={vi.fn()} apis={{ deliver: fakeDeliver() }} />, host));
    const tabs = host.querySelectorAll(".mm-shell-tab");
    expect([...tabs].map((t) => t.textContent)).toEqual(["Tasks", "Deliver"]);
    const panels = host.querySelectorAll<HTMLElement>(".mm-shell-left-body");
    expect(panels[1]?.hidden).toBe(true);
    const link = [...host.querySelectorAll(".mm-tasks button")].find((b) => b.textContent === "Export to a file");
    click(link);
    await act(() => new Promise<void>((ok) => setTimeout(ok, 0)));
    expect(panels[1]?.hidden).toBe(false);
    expect(host.querySelector(".mm-deliver")).not.toBeNull();
  });
});

describe("Shell の Mixer タブ", () => {
  it("Mixer を渡すと Mixer のタブが出て、開くと Mixer のパネルを見せる", async () => {
    const mixer = fakeMixer();
    act(() => render(<Shell sendOp={vi.fn()} apis={{ mixer }} />, host));
    const tabs = host.querySelectorAll(".mm-shell-tab");
    expect([...tabs].map((t) => t.textContent)).toEqual(["Tasks", "Mixer"]);
    const panels = host.querySelectorAll<HTMLElement>(".mm-shell-left-body");
    click(tabs[1]);
    await act(() => new Promise<void>((ok) => setTimeout(ok, 0)));
    expect(panels[1]?.hidden).toBe(false);
    expect(host.querySelector(".mm-mixer")).not.toBeNull();
    expect(mixer.state).toHaveBeenCalled();
  });
});

describe("Shell の Transition タブ", () => {
  it("Transition を渡すとタブが出て、開くとカットを読む", async () => {
    const transition: TransitionApi = {
      cuts: vi.fn().mockResolvedValue({ sequence: null, cuts: [] }),
      kinds: vi.fn().mockResolvedValue([]),
      set: vi.fn(),
      remove: vi.fn(),
    };
    act(() => render(<Shell sendOp={vi.fn()} apis={{ transition }} />, host));
    const tabs = host.querySelectorAll(".mm-shell-tab");
    expect([...tabs].map((t) => t.textContent)).toEqual(["Tasks", "Transition"]);
    click(tabs[1]);
    await act(() => new Promise<void>((ok) => setTimeout(ok, 0)));
    expect(host.querySelector(".mm-tr")).not.toBeNull();
    expect(transition.cuts).toHaveBeenCalled();
  });
});

describe("Shell の右の列", () => {
  it("Review を渡さないときはタブを出さず Inspector だけ", () => {
    act(() => render(<Shell sendOp={vi.fn()} />, host));
    expect(host.querySelector(".mm-shell-right-tabs")).toBeNull();
    expect(host.querySelector(".mm-review")).toBeNull();
  });

  it("Review を渡すと Inspector と Review のタブで切り替え、どちらも作ったまま残す", async () => {
    const review = fakeReview();
    act(() => render(<Shell sendOp={vi.fn()} apis={{ review }} />, host));
    const tabs = host.querySelectorAll(".mm-shell-right-tabs .mm-shell-tab");
    expect([...tabs].map((t) => t.textContent)).toEqual(["Inspector", "Review"]);
    const panels = host.querySelectorAll<HTMLElement>(".mm-shell-right-body");
    expect([panels[0]?.hidden, panels[1]?.hidden]).toEqual([false, true]);
    click(tabs[1]);
    await act(() => new Promise<void>((ok) => setTimeout(ok, 0)));
    expect([panels[0]?.hidden, panels[1]?.hidden]).toEqual([true, false]);
    expect(host.querySelector(".mm-review")).not.toBeNull();
    expect(review.list).toHaveBeenCalled();
  });
});

describe("Shell の外からの変更", () => {
  it("mm:doc-changed が届くと Story と Media が読み直す", async () => {
    let fire: (e: { payload: unknown }) => void = () => undefined;
    const off = vi.fn();
    const listen = vi.fn(async (_event: string, handler: typeof fire) => {
      fire = handler;
      return off;
    });
    const story = fakeStory();
    const media = fakeMedia();
    act(() => render(<Shell sendOp={vi.fn()} apis={{ media, story }} listen={listen} />, host));
    await act(() => new Promise<void>((ok) => setTimeout(ok, 0)));
    expect(listen.mock.calls[0]?.[0]).toBe("mm:doc-changed");
    const before = [vi.mocked(story.overview).mock.calls.length, vi.mocked(media.list).mock.calls.length];
    await act(async () => {
      fire({ payload: { version: 2, txn_id: "x", kind: "op", name: "n", actor: "cli", touched: [] } });
    });
    expect(vi.mocked(story.overview).mock.calls.length).toBe((before[0] ?? 0) + 1);
    expect(vi.mocked(media.list).mock.calls.length).toBe((before[1] ?? 0) + 1);
    act(() => render(null, host));
    expect(off).toHaveBeenCalled();
  });

  it("Tasks の取り込みの項目は Media を開いてファイルの選択を始める", async () => {
    const media = fakeMedia();
    act(() => render(<Shell sendOp={vi.fn()} apis={{ media }} />, host));
    click(host.querySelector(".mm-tasks-link"));
    await act(() => new Promise<void>((ok) => setTimeout(ok, 0)));
    expect(host.querySelectorAll<HTMLElement>(".mm-shell-left-body")[1]?.hidden).toBe(false);
    expect(media.pickFiles).toHaveBeenCalled();
  });
});

describe("Shell", () => {
  it("操作が拒否されたら理由を画面に出す", async () => {
    const sendOp = vi.fn().mockResolvedValue("setParam: no such layer");
    act(() => render(<Shell sendOp={sendOp} />, host));
    act(() => {
      window.dispatchEvent(new CustomEvent(LAYER_SELECTED_EVENT, { detail: { layer } }));
    });
    const opacity = host.querySelector<HTMLInputElement>('.mm-shell-right [aria-label="opacity"]');
    await act(async () => {
      if (opacity) opacity.value = "0.5";
      opacity?.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(host.querySelector(".mm-shell-error")?.textContent).toBe("setParam: no such layer");
  });

  it("右の列の Inspector は選択のイベントでレイヤーを出し、値の変更を sendOp に渡す", () => {
    const sendOp = vi.fn();
    act(() => render(<Shell sendOp={sendOp} />, host));
    expect(host.querySelector(".mm-shell-right")?.textContent).toContain("No layer selected");
    act(() => {
      window.dispatchEvent(new CustomEvent(LAYER_SELECTED_EVENT, { detail: { layer } }));
    });
    const opacity = host.querySelector<HTMLInputElement>('.mm-shell-right [aria-label="opacity"]');
    act(() => {
      if (opacity) opacity.value = "0.5";
      opacity?.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(sendOp).toHaveBeenCalledWith({
      op: "setParam",
      target: { layer: "l1", path: "opacity" },
      value: { type: "number", value: 0.5 },
    });
  });
});
