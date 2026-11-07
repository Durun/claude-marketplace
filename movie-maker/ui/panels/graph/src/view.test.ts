import type { Keyframe, Param } from "@mm/sdk";
import { describe, expect, it } from "vitest";
import { curvePath, dragHandle, dragKey, fitView, handlePoint, secOf, valueOf, xOf, yOf } from "./view.ts";

const sec = (n: number) => ({ num: n, den: 1 });
const num = (t: number, v: number, interpolation: Keyframe["interpolation"] = "linear"): Keyframe => ({
  time: sec(t),
  value: { type: "number", value: v },
  interpolation,
});
const param = (...keyframes: Keyframe[]): Param => ({ value: { type: "number", value: 0 }, keyframes });

describe("fitView", () => {
  it("全部のキーフレームが枠の内側に収まる", () => {
    const p = param(num(1, -2), num(3, 8));
    const v = fitView(p, 1, 200, 100);
    for (const k of p.keyframes ?? []) {
      const x = xOf(v, k.time.num);
      const y = yOf(v, (k.value as { value: number }).value);
      expect(x).toBeGreaterThan(0);
      expect(x).toBeLessThan(200);
      expect(y).toBeGreaterThan(0);
      expect(y).toBeLessThan(100);
    }
  });

  it("秒と値と px の変換は往復で戻る", () => {
    const v = fitView(param(num(0, 0), num(2, 1)), 1, 300, 150);
    expect(secOf(v, xOf(v, 1.25))).toBeCloseTo(1.25, 9);
    expect(valueOf(v, yOf(v, 0.4))).toBeCloseTo(0.4, 9);
  });
});

describe("curvePath", () => {
  it("hold は水平の後に縦の線を描く", () => {
    const p = param(num(0, 0, "hold"), num(1, 1));
    const v = { t0: 0, t1: 1, v0: 0, v1: 1, width: 100, height: 100 };
    expect(curvePath(p, 0, v)).toBe("M0.00,100.00L0.00,100.00L100.00,100.00L100.00,0.00L100.00,0.00");
  });
});

describe("dragKey", () => {
  const keys = [num(0, 0), num(1, 1), num(2, 0)];

  it("時刻はフレームにそろえ、値はそのまま動かす", () => {
    const k = dragKey(keys, 1, 0, { dSec: 0.26, dValue: 0.5, fps: 4 });
    expect(k?.time).toEqual({ num: 5, den: 4 });
    expect(k?.value).toEqual({ type: "number", value: 1.5 });
  });

  it("隣のキーフレームを越えない（1 フレーム手前で止まる）", () => {
    expect(dragKey(keys, 1, 0, { dSec: 5, dValue: 0, fps: 10 })?.time).toEqual({ num: 19, den: 10 });
    expect(dragKey(keys, 1, 0, { dSec: -5, dValue: 0, fps: 10 })?.time).toEqual({ num: 1, den: 10 });
  });

  it("Shift で 1 つの向きだけ動かす", () => {
    const h = dragKey(keys, 1, 0, { dSec: 0.5, dValue: 3, fps: 2, constrain: { horizontal: true } });
    expect(h?.value).toEqual({ type: "number", value: 1 });
    const vert = dragKey(keys, 1, 0, { dSec: 0.5, dValue: 3, fps: 2, constrain: { horizontal: false } });
    expect(vert?.time).toEqual(sec(1));
  });

  it("2 次元の数は掴んだ成分だけ変える", () => {
    const k: Keyframe = { time: sec(0), value: { type: "vec2", value: [1, 2] }, interpolation: "linear" };
    expect(dragKey([k], 0, 1, { dSec: 0, dValue: 3, fps: 30 })?.value).toEqual({ type: "vec2", value: [1, 5] });
  });
});

describe("dragHandle", () => {
  const keys = [num(0, 0, "bezier"), num(2, 10)];

  it("ベジェのハンドルの位置は区間を正規化した cubic-bezier の制御点", () => {
    const b = dragHandle(keys, 0, "out", 0, { sec: 0.5, value: 8 });
    expect(b).toEqual({ x1: 0.25, y1: 0.8, x2: 0.58, y2: 1 });
    const from: Keyframe = { ...num(0, 0, "bezier"), bezier: b };
    expect(handlePoint(from, num(2, 10), "out", 0)).toEqual([0.5, 8]);
  });

  it("x は区間の外へ出さない（時間が単調）", () => {
    expect(dragHandle(keys, 0, "in", 0, { sec: 9, value: 5 })?.x2).toBe(1);
    expect(dragHandle(keys, 0, "out", 0, { sec: -1, value: 5 })?.x1).toBe(0);
  });

  it("両端の値が同じ区間では y を変えない", () => {
    const flat = [num(0, 3, "bezier"), num(1, 3)];
    expect(dragHandle(flat, 0, "out", 0, { sec: 0.3, value: 99 })?.y1).toBe(0);
  });
});
