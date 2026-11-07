// @vitest-environment jsdom
import { render } from "preact";
import { act } from "preact/test-utils";
import type { Keyframe, Layer, Op } from "@mm/sdk";
import { afterEach, describe, expect, it, vi } from "vitest";
import { InspectorPanel, type InspectorPanelProps } from "./InspectorPanel.tsx";
import { activeKey } from "./keyframes.ts";

const host = document.createElement("div");
document.body.append(host);
afterEach(() => render(null, host));

const t = (num: number, den = 1) => ({ num, den });
const key = (time: number, value: number, extra: Partial<Keyframe> = {}): Keyframe => ({
  time: t(time),
  value: { type: "number", value },
  interpolation: "linear",
  ...extra,
});
const bezier = { x1: 0.1, y1: 0.2, x2: 0.3, y2: 0.4 };

const layer = {
  id: "l1",
  start: t(0),
  in: t(0),
  out: t(4),
  opacity: { value: { type: "number", value: 1 }, keyframes: [key(0, 0), key(2, 1, { interpolation: "bezier", bezier })] },
  transform: {},
  source: { kind: "demo", version: 1, params: {} },
} as unknown as Layer;

const mount = (props: Partial<InspectorPanelProps> & { onOp: (op: Op) => void }) =>
  render(<InspectorPanel layer={layer} fps={10} {...props} />, host);

const el = <T extends Element>(label: string) => host.querySelector<T>(`[aria-label="${label}"]`);
const click = (label: string) => act(() => el<HTMLButtonElement>(label)?.click());

describe("Inspector のキーフレームナビゲーター", () => {
  it("再生位置を渡さなければキーフレームナビゲーターを出さず、動きのある値は編集できない", () => {
    mount({ onOp: vi.fn() });
    expect(el("opacity keyframe at playhead")).toBeNull();
    expect(el<HTMLInputElement>("opacity")?.disabled).toBe(true);
  });

  it("再生位置のキーの有無をひし形で示し、押すと外す（removeKeyframe）", () => {
    const onOp = vi.fn();
    mount({ onOp, time: 2.01 });
    expect(el("opacity keyframe at playhead")?.getAttribute("aria-pressed")).toBe("true");
    click("opacity keyframe at playhead");
    expect(onOp).toHaveBeenCalledWith({ op: "removeKeyframe", target: { layer: "l1", path: "opacity" }, time: t(2) });
  });

  it("キーの無い位置で押すと、今の値でフレームの先頭に足す（setKeyframe）", () => {
    const onOp = vi.fn();
    mount({ onOp, time: 1.04 });
    expect(el("opacity keyframe at playhead")?.getAttribute("aria-pressed")).toBe("false");
    click("opacity keyframe at playhead");
    const op = onOp.mock.calls[0]?.[0] as Op;
    expect(op).toMatchObject({ op: "setKeyframe", keyframe: { time: t(1), interpolation: "linear" } });
    if (op.op === "setKeyframe" && op.keyframe.value.type === "number") expect(op.keyframe.value.value).toBeCloseTo(0.5, 9);
  });

  it("前後のキーフレームへ再生位置を動かし、端ではボタンを止める", () => {
    const onSeek = vi.fn();
    mount({ onOp: vi.fn(), time: 1, onSeek });
    click("opacity previous keyframe");
    click("opacity next keyframe");
    expect(onSeek.mock.calls).toEqual([[0], [2]]);
    mount({ onOp: vi.fn(), time: 3, onSeek });
    expect(el<HTMLButtonElement>("opacity next keyframe")?.disabled).toBe(true);
  });
});

describe("Inspector の補間と値の編集", () => {
  it("補間の切り替えは区間の始点に setEase を送り、ベジェの制御点を残す", () => {
    const onOp = vi.fn();
    mount({ onOp, time: 3 });
    const select = el<HTMLSelectElement>("opacity interpolation");
    expect(select?.value).toBe("bezier");
    act(() => {
      if (select) select.value = "hold";
      select?.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(onOp).toHaveBeenCalledWith({
      op: "setEase",
      target: { layer: "l1", path: "opacity" },
      time: t(2),
      interpolation: "hold",
      ease: undefined,
      bezier,
    });
  });

  it("動きのある値を入力欄で変えると、再生位置のキーフレームの値を変える", () => {
    const onOp = vi.fn();
    mount({ onOp, time: 2 });
    const input = el<HTMLInputElement>("opacity");
    expect(input?.disabled).toBe(false);
    act(() => {
      if (input) input.value = "0.75";
      input?.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(onOp).toHaveBeenCalledWith({
      op: "setKeyframe",
      target: { layer: "l1", path: "opacity" },
      keyframe: { ...key(2, 0.75), interpolation: "bezier", bezier },
    });
  });

  it("名前を押すと Graph に出すパラメータを選ぶ", () => {
    const onSelectParam = vi.fn();
    mount({ onOp: vi.fn(), time: 0, onSelectParam, selectedPath: "opacity" });
    act(() => host.querySelector<HTMLButtonElement>(".mm-insp-pick")?.click());
    expect(onSelectParam).toHaveBeenCalledWith("opacity");
    expect(host.querySelector("[data-selected=true]")).not.toBeNull();
  });
});

describe("activeKey", () => {
  it("再生位置にあればそれ、無ければ直前のキーフレーム", () => {
    const p = layer.opacity;
    expect(activeKey(p, { sec: 2, fps: 10 })?.time).toEqual(t(2));
    expect(activeKey(p, { sec: 1.5, fps: 10 })?.time).toEqual(t(0));
    expect(activeKey({ ...p, keyframes: [key(1, 0)] }, { sec: 0.5, fps: 10 })).toBeNull();
  });
});
