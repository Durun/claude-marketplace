// @vitest-environment jsdom
import { render } from "preact";
import { act } from "preact/test-utils";
import type { Layer, Op } from "@mm/sdk";
import { afterEach, describe, expect, it, vi } from "vitest";
import { InspectorPanel } from "./InspectorPanel.tsx";
import { nextMaskId } from "./maskOps.ts";
import { describeLayer, setParamOp } from "./model.ts";

const host = document.createElement("div");
document.body.append(host);
afterEach(() => render(null, host));

const t0 = { num: 0, den: 1 };
const num = (value: number) => ({ value: { type: "number" as const, value } });
const vec2 = (x: number, y: number) => ({ value: { type: "vec2" as const, value: [x, y] as [number, number] } });

const layer = {
  id: "l1",
  start: t0,
  in: t0,
  out: { num: 4, den: 1 },
  opacity: num(1),
  transform: {},
  source: { kind: "demo", version: 1, params: {} },
  crop: { left: num(10) },
  masks: [{ id: "mask-1", rect: { position: vec2(5, 6), size: vec2(100, 50), radius: num(8) } }],
  audio: {
    role: "voice",
    gain_db: num(0),
    pan: num(0),
    fade_in: { duration: { num: 500, den: 1000 }, curve: "equal_power" },
  },
} as unknown as Layer;

const mount = (onOp: (op: Op) => void, l: Layer = layer) => render(<InspectorPanel layer={l} onOp={onOp} />, host);
const el = (label: string) => host.querySelector<HTMLInputElement & HTMLSelectElement>(`[aria-label="${label}"]`);
const change = (label: string, set: (e: HTMLInputElement & HTMLSelectElement) => void) => {
  const e = el(label);
  act(() => {
    set(e as HTMLInputElement & HTMLSelectElement);
    e?.dispatchEvent(new Event("change", { bubbles: true }));
  });
};

describe("切り抜き", () => {
  it("4 辺の行を crop.<辺> のパスで出し、無い辺は 0", () => {
    const crop = describeLayer(layer).find((s) => s.id === "crop");
    expect(crop?.rows.map((r) => [r.path, r.param.value])).toEqual([
      ["crop.left", { type: "number", value: 10 }],
      ["crop.top", { type: "number", value: 0 }],
      ["crop.right", { type: "number", value: 0 }],
      ["crop.bottom", { type: "number", value: 0 }],
    ]);
  });

  it("値を変えると setParam を 1 回送る", () => {
    const onOp = vi.fn();
    mount(onOp);
    change("top", (e) => (e.value = "12"));
    expect(onOp).toHaveBeenCalledTimes(1);
    expect(onOp).toHaveBeenCalledWith(setParamOp("l1", "crop.top", { type: "number", value: 12 }));
  });

  it("再生位置があると、4 辺にキーフレームのボタンが付く", () => {
    render(<InspectorPanel layer={layer} onOp={vi.fn()} time={1} />, host);
    expect(el("left keyframe at playhead")).not.toBeNull();
    expect(el("bottom keyframe at playhead")).not.toBeNull();
  });
});

describe("マスク", () => {
  it("位置・大きさ・半径の行を masks.<ID>.rect.* で出す", () => {
    const s = describeLayer(layer).find((x) => x.id === "mask-mask-1");
    expect(s?.rows.map((r) => r.path)).toEqual([
      "masks.mask-1.rect.position",
      "masks.mask-1.rect.size",
      "masks.mask-1.rect.radius",
    ]);
  });

  it("半径を変えると setParam を 1 回送る", () => {
    const onOp = vi.fn();
    mount(onOp);
    change("radius", (e) => (e.value = "20"));
    expect(onOp).toHaveBeenCalledTimes(1);
    expect(onOp).toHaveBeenCalledWith(setParamOp("l1", "masks.mask-1.rect.radius", { type: "number", value: 20 }));
  });

  it("追加は addMask を 1 回、ID は重ならない", () => {
    const onOp = vi.fn();
    mount(onOp);
    act(() => el("add rect mask")?.click());
    expect(onOp).toHaveBeenCalledTimes(1);
    const op = onOp.mock.calls[0]?.[0] as Extract<Op, { op: "addMask" }>;
    expect(op).toMatchObject({ op: "addMask", layer: "l1", mask: { id: "mask-2" } });
    expect(op.mask.rect?.radius?.value).toEqual({ type: "number", value: 24 });
    expect(nextMaskId([{ id: "mask-2" }, { id: "mask-1" }] as Layer["masks"] & object)).toBe("mask-3");
  });

  it("削除は removeMask を 1 回", () => {
    const onOp = vi.fn();
    mount(onOp);
    act(() => el("remove mask-1")?.click());
    expect(onOp.mock.calls).toEqual([[{ op: "removeMask", mask: "mask-1" }]]);
  });
});

describe("音のフェード", () => {
  it("長さを秒で変えると、曲線を保った setLayerAudio を 1 回送る", () => {
    const onOp = vi.fn();
    mount(onOp);
    expect(el("fade in seconds")?.value).toBe("0.5");
    change("fade in seconds", (e) => (e.value = "1.25"));
    expect(onOp).toHaveBeenCalledTimes(1);
    const op = onOp.mock.calls[0]?.[0] as Extract<Op, { op: "setLayerAudio" }>;
    expect(op.audio?.fade_in).toEqual({ duration: { num: 1250, den: 1000 }, curve: "equal_power" });
    expect(op.audio?.gain_db).toEqual(num(0));
  });

  it("フェードの無い側に長さを入れると、既定の曲線で足す", () => {
    const onOp = vi.fn();
    mount(onOp);
    expect(el("fade out curve")?.disabled).toBe(true);
    change("fade out seconds", (e) => (e.value = "2"));
    const op = onOp.mock.calls[0]?.[0] as Extract<Op, { op: "setLayerAudio" }>;
    expect(op.audio?.fade_out).toEqual({ duration: { num: 2000, den: 1000 } });
  });

  it("長さを 0 にすると外す", () => {
    const onOp = vi.fn();
    mount(onOp);
    change("fade in seconds", (e) => (e.value = "0"));
    const op = onOp.mock.calls[0]?.[0] as Extract<Op, { op: "setLayerAudio" }>;
    expect(op.audio && "fade_in" in op.audio).toBe(false);
  });

  it("曲線を選ぶと setLayerAudio を 1 回送る", () => {
    const onOp = vi.fn();
    mount(onOp);
    change("fade in curve", (e) => (e.value = "db"));
    expect(onOp).toHaveBeenCalledTimes(1);
    const op = onOp.mock.calls[0]?.[0] as Extract<Op, { op: "setLayerAudio" }>;
    expect(op.audio?.fade_in?.curve).toBe("db");
  });
});
