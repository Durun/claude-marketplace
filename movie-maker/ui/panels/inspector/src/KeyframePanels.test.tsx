// @vitest-environment jsdom
import { render } from "preact";
import { act } from "preact/test-utils";
import type { Layer } from "@mm/sdk";
import { afterEach, describe, expect, it, vi } from "vitest";
import { KeyframePanels, PLAYHEAD_EVENT, SEEK_EVENT } from "./KeyframePanels.tsx";

const host = document.createElement("div");
document.body.append(host);
afterEach(() => render(null, host));

const t = (num: number) => ({ num, den: 1 });
const layer = {
  id: "l1",
  start: t(0),
  in: t(0),
  out: t(4),
  opacity: {
    value: { type: "number", value: 1 },
    keyframes: [
      { time: t(0), value: { type: "number", value: 0 }, interpolation: "linear" },
      { time: t(2), value: { type: "number", value: 1 }, interpolation: "linear" },
    ],
  },
  transform: {},
  source: { kind: "demo", version: 1, params: {} },
} as unknown as Layer;

describe("KeyframePanels", () => {
  it("再生位置のイベントでキーフレームナビゲーターを出し、前後の移動はシークのイベントにする", () => {
    act(() => render(<KeyframePanels layer={layer} onOp={vi.fn()} />, host));
    expect(host.querySelector('[aria-label="opacity keyframe at playhead"]')).toBeNull();
    act(() => {
      window.dispatchEvent(new CustomEvent(PLAYHEAD_EVENT, { detail: { time: 1, fps: 30 } }));
    });
    const seeks: number[] = [];
    const onSeek = (e: Event) => seeks.push((e as CustomEvent<{ time: number }>).detail.time);
    window.addEventListener(SEEK_EVENT, onSeek);
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="opacity next keyframe"]')?.click());
    window.removeEventListener(SEEK_EVENT, onSeek);
    expect(seeks).toEqual([2]);
  });

  it("止まっている間に同じ再生位置が届いても、入力中の入力欄を戻さない", () => {
    const playhead = () => {
      window.dispatchEvent(new CustomEvent(PLAYHEAD_EVENT, { detail: { time: 1, fps: 30 } }));
    };
    act(() => render(<KeyframePanels layer={layer} onOp={vi.fn()} />, host));
    act(playhead);
    const input = host.querySelector<HTMLInputElement>('input[aria-label="opacity"]');
    if (!input) throw new Error("opacity の入力欄が無い");
    input.value = "0.75";
    act(() => void input.dispatchEvent(new Event("input", { bubbles: true })));
    act(playhead);
    expect(input.value).toBe("0.75");
  });

  it("Inspector で名前を押したパラメータを Graph に出す", () => {
    act(() => render(<KeyframePanels layer={layer} onOp={vi.fn()} />, host));
    expect(host.querySelector(".mm-graph")?.textContent).toContain("Select a parameter");
    act(() => host.querySelector<HTMLButtonElement>(".mm-insp-pick")?.click());
    expect(host.querySelector(".mm-graph-title")?.textContent).toBe("Graph: opacity");
    expect(host.querySelectorAll(".mm-graph-key")).toHaveLength(2);
  });
});
