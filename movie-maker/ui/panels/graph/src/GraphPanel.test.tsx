// @vitest-environment jsdom
import { render } from "preact";
import { act } from "preact/test-utils";
import type { Keyframe, Op, Param } from "@mm/sdk";
import { afterEach, describe, expect, it, vi } from "vitest";
import { GraphPanel } from "./GraphPanel.tsx";
import { fitView, xOf, yOf } from "./view.ts";

const host = document.createElement("div");
document.body.append(host);
afterEach(() => render(null, host));

const W = 200;
const H = 100;
const target = { layer: "l1", path: "opacity" };
const sec = (n: number) => ({ num: n, den: 1 });
const num = (t: number, v: number, interpolation: Keyframe["interpolation"] = "linear"): Keyframe => ({
  time: sec(t),
  value: { type: "number", value: v },
  interpolation,
});
const param: Param = { value: { type: "number", value: 0 }, keyframes: [num(0, 0, "bezier"), num(2, 1)] };
const view = fitView(param, 1, W, H);

const mount = (onOp: (op: Op) => void, onPreview?: (p: Param | null) => void, p: Param | null = param) =>
  render(
    <GraphPanel target={target} param={p} label="opacity" fps={10} width={W} height={H} onOp={onOp} onPreview={onPreview} />,
    host,
  );

// jsdom の要素には onpointerdown などが無く、preact はそのとき名前の大小を保って購読する（PointerDown）。
const EVENT_NAMES: Record<string, string> = { pointerdown: "PointerDown", pointermove: "PointerMove", pointerup: "PointerUp" };

const pointer = (el: Element | null, type: string, x: number, y: number, shiftKey = false) =>
  act(() => {
    const name = el && `on${type}` in el ? type : (EVENT_NAMES[type] ?? type);
    el?.dispatchEvent(new MouseEvent(name, { clientX: x, clientY: y, bubbles: true, shiftKey }));
  });

const svg = () => host.querySelector("svg");
const key = (label: string) => host.querySelector(`[aria-label="${label}"]`);

describe("GraphPanel", () => {
  it("パラメータが無い・数でないときは案内を出す", () => {
    mount(vi.fn(), undefined, null);
    expect(host.textContent).toContain("Select a parameter");
    mount(vi.fn(), undefined, { value: { type: "text", value: "a" } });
    expect(host.textContent).toContain("no curve");
  });

  it("再生位置のキーフレームを示す", () => {
    render(<GraphPanel target={target} param={param} time={2} width={W} height={H} onOp={vi.fn()} />, host);
    expect(key("key 2.000s x")?.getAttribute("data-current")).toBe("true");
    expect(key("key 0.000s x")?.getAttribute("data-current")).toBe("false");
  });
});

describe("GraphPanel の取りやめと再生位置", () => {
  it("Esc で取りやめると元の曲線に戻り、離しても操作を送らない", () => {
    const onOp = vi.fn();
    const onPreview = vi.fn();
    mount(onOp, onPreview);
    const before = host.querySelector("path")?.getAttribute("d");
    pointer(key("key 2.000s x"), "pointerdown", xOf(view, 2), yOf(view, 1));
    pointer(svg(), "pointermove", xOf(view, 1.5), yOf(view, 0.5));
    act(() => void window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })));
    expect(host.querySelector("path")?.getAttribute("d")).toBe(before);
    expect(onPreview).toHaveBeenLastCalledWith(null);
    pointer(svg(), "pointerup", xOf(view, 1.5), yOf(view, 0.5));
    expect(onOp).not.toHaveBeenCalled();
  });

  it("再生位置のキーフレームはフレームで比べる（Inspector の ◆ と同じ）", () => {
    render(<GraphPanel target={target} param={param} time={2.04} fps={10} width={W} height={H} onOp={vi.fn()} />, host);
    expect(key("key 2.000s x")?.getAttribute("data-current")).toBe("true");
  });
});

describe("GraphPanel のドラッグ", () => {
  it("キーフレームのドラッグは途中を見せ、離したら moveKeyframe を 1 つ送る", () => {
    const onOp = vi.fn();
    const onPreview = vi.fn();
    mount(onOp, onPreview);
    const before = host.querySelector("path")?.getAttribute("d");
    const [x, y] = [xOf(view, 2), yOf(view, 1)];
    pointer(key("key 2.000s x"), "pointerdown", x, y);
    pointer(svg(), "pointermove", xOf(view, 1.5), yOf(view, 0.5));
    expect(host.querySelector("path")?.getAttribute("d")).not.toBe(before);
    expect(onPreview).toHaveBeenLastCalledWith(expect.objectContaining({ keyframes: expect.any(Array) }));
    expect(onOp).not.toHaveBeenCalled();
    pointer(svg(), "pointerup", xOf(view, 1.5), yOf(view, 0.5));
    expect(onPreview).toHaveBeenLastCalledWith(null);
    expect(onOp).toHaveBeenCalledTimes(1);
    const op = onOp.mock.calls[0]?.[0] as Op;
    expect(op).toMatchObject({ op: "moveKeyframe", target, from: sec(2), keyframe: { time: { num: 3, den: 2 } } });
    if (op.op === "moveKeyframe" && op.keyframe.value.type === "number") {
      expect(op.keyframe.value.value).toBeCloseTo(0.5, 6);
    }
  });

  it("Shift のドラッグは大きく動いた向きだけを使う", () => {
    const onOp = vi.fn();
    mount(onOp);
    pointer(key("key 2.000s x"), "pointerdown", xOf(view, 2), yOf(view, 1));
    pointer(svg(), "pointermove", xOf(view, 1), yOf(view, 0.9), true);
    pointer(svg(), "pointerup", xOf(view, 1), yOf(view, 0.9), true);
    expect(onOp.mock.calls[0]?.[0]).toMatchObject({ keyframe: { time: sec(1), value: { value: 1 } } });
  });

  it("ベジェのハンドルのドラッグは setEase（bezier の制御点）を 1 つ送る", () => {
    const onOp = vi.fn();
    mount(onOp);
    pointer(key("handle 0 out x"), "pointerdown", xOf(view, 0.84), yOf(view, 0));
    pointer(svg(), "pointermove", xOf(view, 0.5), yOf(view, 0.8));
    pointer(svg(), "pointerup", xOf(view, 0.5), yOf(view, 0.8));
    expect(onOp).toHaveBeenCalledTimes(1);
    const op = onOp.mock.calls[0]?.[0] as Op;
    expect(op).toMatchObject({ op: "setEase", target, time: sec(0), interpolation: "bezier" });
    if (op.op === "setEase") {
      expect(op.bezier?.x1).toBeCloseTo(0.25, 6);
      expect(op.bezier?.y1).toBeCloseTo(0.8, 6);
    }
  });
});

describe("GraphPanel のクリック", () => {
  it("動かさずに離したら何も送らない", () => {
    const onOp = vi.fn();
    mount(onOp);
    pointer(key("key 0.000s x"), "pointerdown", 10, 10);
    pointer(svg(), "pointerup", 10, 10);
    expect(onOp).not.toHaveBeenCalled();
  });
});
