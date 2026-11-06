// @vitest-environment jsdom
import { render } from "preact";
import { act } from "preact/test-utils";
import type { Layer, MediaApi, StoryApi } from "@mm/sdk";
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
  apply: vi.fn().mockResolvedValue({ ok: true, value: {} }),
  seek: vi.fn(),
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
    act(() => render(<Shell sendOp={vi.fn()} media={fakeMedia()} />, host));
    const panels = host.querySelectorAll<HTMLElement>(".mm-shell-left-body");
    expect([panels[0]?.hidden, panels[1]?.hidden]).toEqual([false, true]);
    click(host.querySelectorAll(".mm-shell-tab")[1]);
    expect([panels[0]?.hidden, panels[1]?.hidden]).toEqual([true, false]);
    expect(host.querySelector(".mm-media")).not.toBeNull();
  });

  it("Story を渡すと Story のタブが出て、開くたびに読み直す", async () => {
    const story = fakeStory();
    act(() => render(<Shell sendOp={vi.fn()} media={fakeMedia()} story={story} />, host));
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

  it("Tasks の取り込みの項目は Media を開いてファイルの選択を始める", async () => {
    const media = fakeMedia();
    act(() => render(<Shell sendOp={vi.fn()} media={media} />, host));
    click(host.querySelector(".mm-tasks-link"));
    await act(() => new Promise<void>((ok) => setTimeout(ok, 0)));
    expect(host.querySelectorAll<HTMLElement>(".mm-shell-left-body")[1]?.hidden).toBe(false);
    expect(media.pickFiles).toHaveBeenCalled();
  });
});

describe("Shell", () => {
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
