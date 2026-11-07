// @vitest-environment jsdom
import { render } from "preact";
import { act } from "preact/test-utils";
import { PARAM_VALUE_SCHEMAS, type Asset, type Layer, type Op } from "@mm/sdk";
import { afterEach, describe, expect, it, vi } from "vitest";
import { hexToRgba, rgbaToHex } from "./color.ts";
import { fieldSpecFor, type ParamKind } from "./fields.ts";
import { InspectorPanel } from "./InspectorPanel.tsx";
import { describeLayer, setParamOp } from "./model.ts";

const host = document.createElement("div");
document.body.append(host);
afterEach(() => render(null, host));

const t0 = { num: 0, den: 1 };
const fixed = (value: Layer["opacity"]["value"]) => ({ value });

const layer: Layer = {
  id: "l1",
  start: t0,
  in: t0,
  out: { num: 1, den: 1 },
  opacity: {
    value: { type: "number", value: 1 },
    keyframes: [{ time: t0, value: { type: "number", value: 0.25 }, interpolation: "linear" }],
  },
  transform: { position: fixed({ type: "vec2", value: [10, 20] }) },
  source: {
    kind: "demo",
    version: 1,
    params: {
      n: fixed({ type: "number", value: 2 }),
      on: fixed({ type: "bool", value: false }),
      name: fixed({ type: "text", value: "hi" }),
      pick: fixed({ type: "choice", value: "b" }),
      asset: fixed({ type: "asset_ref", value: "a1" }),
      tint: fixed({ type: "color", value: { r: 0.5, g: 0.5, b: 0.5, a: 1 } }),
      vec: fixed({ type: "vec3", value: [1, 2, 3] }),
      raw: fixed({ type: "json", value: { k: 1 } }),
    },
  },
} as Layer;

const assets = [{ id: "a1" }, { id: "a2" }] as Asset[];

const mount = (onOp: (op: Op) => void) =>
  render(<InspectorPanel layer={layer} assets={assets} hints={{ "source.pick": { choices: ["a", "b"] } }} onOp={onOp} />, host);

const input = (label: string) => host.querySelector<HTMLInputElement | HTMLSelectElement>(`[aria-label="${label}"]`);

const change = (el: HTMLInputElement | HTMLSelectElement | null, set: (e: HTMLInputElement & HTMLSelectElement) => void) => {
  act(() => {
    set(el as HTMLInputElement & HTMLSelectElement);
    el?.dispatchEvent(new Event("change", { bubbles: true }));
  });
};

describe("fieldSpecFor", () => {
  it("ParamValue の種類を全部、JSON Schema から入力欄の種類にする", () => {
    const specs = Object.keys(PARAM_VALUE_SCHEMAS).map((k) => [k, fieldSpecFor(k as ParamKind).control]);
    expect(Object.fromEntries(specs)).toEqual({
      number: "number",
      vec2: "vector",
      vec3: "vector",
      color: "color",
      bool: "bool",
      text: "text",
      choice: "choice",
      asset_ref: "asset",
      json: "json",
    });
    expect(fieldSpecFor("vec3")).toEqual({ control: "vector", size: 3 });
  });
});

describe("describeLayer", () => {
  it("区画の順に、パスつきの行を作る", () => {
    const sections = describeLayer(layer);
    expect(sections.map((s) => s.id)).toEqual(["layer", "transform", "crop", "source"]);
    expect(sections[3]?.rows.map((r) => r.path)).toContain("source.tint");
  });

  it("setParam の操作を作る", () => {
    expect(setParamOp("l1", "opacity", { type: "number", value: 0.5 })).toEqual({
      op: "setParam",
      target: { layer: "l1", path: "opacity" },
      value: { type: "number", value: 0.5 },
    });
  });
});

describe("color", () => {
  it("線形と表示の値を往復する", () => {
    const hex = rgbaToHex({ r: 1, g: 0, b: 0.1913, a: 1 });
    expect(hex).toBe("#ff0080");
    const back = hexToRgba(hex, 1);
    expect(back?.b).toBeCloseTo(0.1913, 3);
  });

  it("Program と同じ Rec.1886 の式で、選んだ色がそのまま画面に出る", () => {
    // Program は線形の値を v ** (1 / 2.4) にして出す。選んだ #101010 はその逆で線形にする。
    const linear = hexToRgba("#101010", 1);
    expect(linear?.r).toBeCloseTo((16 / 255) ** 2.4, 6);
    const shown = Math.round(255 * (linear?.r ?? 0) ** (1 / 2.4));
    expect(shown).toBe(16);
    expect(rgbaToHex(linear ?? { r: 0, g: 0, b: 0, a: 1 })).toBe("#101010");
  });

  it("premultiplied の値は straight にして表示する", () => {
    expect(rgbaToHex({ r: 0.5, g: 0, b: 0, a: 0.5 })).toBe("#ff0000");
  });
});

describe("InspectorPanel", () => {
  it("選んだレイヤーが無いと空の表示", () => {
    render(<InspectorPanel layer={null} onOp={vi.fn()} />, host);
    expect(host.textContent).toContain("No layer selected");
  });

  it("キーフレームのあるパラメータはマーカーを出し、編集できない", () => {
    mount(vi.fn());
    expect(host.querySelectorAll(".mm-insp-kf").length).toBe(1);
    expect(input("opacity")?.disabled).toBe(true);
    expect(input("opacity")?.value).toBe("0.25");
    expect(input("position x")?.disabled).toBe(false);
  });

  it("数を変えると setParam を送る", () => {
    const onOp = vi.fn();
    mount(onOp);
    change(input("n"), (e) => (e.value = "7"));
    expect(onOp).toHaveBeenCalledWith(setParamOp("l1", "source.n", { type: "number", value: 7 }));
  });

  it("2 次元の片方を変えると、もう片方を保って送る", () => {
    const onOp = vi.fn();
    mount(onOp);
    change(input("position y"), (e) => (e.value = "99"));
    expect(onOp).toHaveBeenCalledWith(setParamOp("l1", "transform.position", { type: "vec2", value: [10, 99] }));
  });

});

describe("InspectorPanel の入力欄", () => {
  it("真偽、文字、選択肢、素材の参照、3 次元の値を送る", () => {
    const onOp = vi.fn();
    mount(onOp);
    change(input("on"), (e) => (e.checked = true));
    change(input("name"), (e) => (e.value = "yo"));
    change(input("pick"), (e) => (e.value = "a"));
    change(input("asset"), (e) => (e.value = "a2"));
    change(input("vec z"), (e) => (e.value = "9"));
    const sent = onOp.mock.calls.map(([op]) => (op as Extract<Op, { op: "setParam" }>).value);
    expect(sent).toEqual([
      { type: "bool", value: true },
      { type: "text", value: "yo" },
      { type: "choice", value: "a" },
      { type: "asset_ref", value: "a2" },
      { type: "vec3", value: [1, 2, 9] },
    ]);
  });

  it("色の透明度を変えても、見た目の色を保つ", () => {
    const onOp = vi.fn();
    mount(onOp);
    change(input("tint alpha"), (e) => (e.value = "0.5"));
    const op = onOp.mock.calls[0]?.[0] as Extract<Op, { op: "setParam" }>;
    expect(op.value).toEqual({ type: "color", value: { r: 0.25, g: 0.25, b: 0.25, a: 0.5 } });
  });

  it("json の値は読み取りだけ", () => {
    mount(vi.fn());
    expect(host.querySelector(".mm-insp-json")?.textContent).toBe('{"k":1}');
  });
});
