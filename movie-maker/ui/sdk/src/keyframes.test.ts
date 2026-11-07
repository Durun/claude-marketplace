import type { Keyframe, Op, Param } from "./generated/ops.ts";
import { describe, expect, it } from "vitest";
import {
  applyKeyframeOp,
  bezierY,
  evalParam,
  EASE_IN_OUT,
  fpsRational,
  frameTime,
  keyIndexAt,
  nextKeySec,
  prevKeySec,
  sameTime,
} from "./keyframes.ts";

const sec = (n: number) => ({ num: n, den: 1 });
const num = (t: number, v: number, interpolation: Keyframe["interpolation"] = "linear"): Keyframe => ({
  time: sec(t),
  value: { type: "number", value: v },
  interpolation,
});
const param = (...keyframes: Keyframe[]): Param => ({ value: { type: "number", value: 9 }, keyframes });
const at = (p: Param, t: number) => (evalParam(p, t) as { value: number }).value;

describe("frameTime", () => {
  it("秒をいちばん近いフレームの先頭にそろえ、既約の有理数にする", () => {
    expect(frameTime(1.01, 30)).toEqual({ num: 1, den: 1 });
    expect(frameTime(0.5, 30)).toEqual({ num: 1, den: 2 });
    expect(frameTime(1 / 30 + 0.001, 30)).toEqual({ num: 1, den: 30 });
  });

  it("NTSC 系の fps は x/1001 で数える", () => {
    expect(fpsRational(29.97002997)).toEqual({ num: 30000, den: 1001 });
    expect(frameTime(1001 / 30000, 30000 / 1001)).toEqual({ num: 1001, den: 30000 });
  });

  it("分母の違う同じ時刻を同じとみなす", () => {
    expect(sameTime({ num: 2, den: 4 }, { num: 1, den: 2 })).toBe(true);
  });
});

describe("evalParam（mm-eval と同じ決まり）", () => {
  it("キーフレームが無ければ value、最初より前と最後より後は端の値", () => {
    expect(at({ value: { type: "number", value: 9 } }, 3)).toBe(9);
    const p = param(num(1, 0), num(3, 10));
    expect(at(p, 0)).toBe(0);
    expect(at(p, 5)).toBe(10);
  });

  it("linear は直線、hold は次まで保つ", () => {
    expect(at(param(num(0, 0), num(2, 10)), 1)).toBe(5);
    expect(at(param(num(0, 0, "hold"), num(2, 10)), 1.9)).toBe(0);
  });

  it("bezier は既定で ease-in-out（真ん中で半分、端の近くは遅い）", () => {
    const p = param(num(0, 0, "bezier"), num(2, 10));
    expect(at(p, 1)).toBeCloseTo(5, 6);
    expect(at(p, 0.2)).toBeLessThan(1);
  });

  it("2 次元の数は成分ごとに補間する", () => {
    const p: Param = {
      value: { type: "vec2", value: [0, 0] },
      keyframes: [
        { time: sec(0), value: { type: "vec2", value: [0, 10] }, interpolation: "linear" },
        { time: sec(1), value: { type: "vec2", value: [4, 0] }, interpolation: "linear" },
      ],
    };
    expect(evalParam(p, 0.5)).toEqual({ type: "vec2", value: [2, 5] });
  });

  it("cubic-bezier の y は端で 0 と 1、x と同じ制御点なら直線", () => {
    expect(bezierY(EASE_IN_OUT, 0)).toBe(0);
    expect(bezierY(EASE_IN_OUT, 1)).toBe(1);
    expect(bezierY({ x1: 0.25, y1: 0.25, x2: 0.75, y2: 0.75 }, 0.3)).toBeCloseTo(0.3, 6);
  });
});

describe("前後のキーフレーム", () => {
  const p = param(num(0, 0), num(1, 1), num(2, 2));

  it("再生位置のフレームのキーフレームを見つける", () => {
    expect(keyIndexAt(p, 1.01, 30)).toBe(1);
    expect(keyIndexAt(p, 1.5, 30)).toBe(-1);
  });

  it("前と次はフレームで比べ、端では null", () => {
    expect(prevKeySec(p, 1.01, 30)).toBe(0);
    expect(nextKeySec(p, 1.01, 30)).toBe(2);
    expect(prevKeySec(p, 0, 30)).toBeNull();
    expect(nextKeySec(p, 2, 30)).toBeNull();
  });
});

describe("applyKeyframeOp（mm-ops と同じ結果）", () => {
  const target = { layer: "l1", path: "opacity" };
  const p = param(num(0, 0), num(2, 1));

  it("setKeyframe は時刻の順に差し込み、同じ時刻は置き換える", () => {
    const added = applyKeyframeOp(p, { op: "setKeyframe", target, keyframe: num(1, 5) });
    expect(added.keyframes?.map((k) => k.time.num)).toEqual([0, 1, 2]);
    const replaced = applyKeyframeOp(p, { op: "setKeyframe", target, keyframe: num(2, 7) });
    expect(replaced.keyframes?.[1]?.value).toEqual({ type: "number", value: 7 });
  });

  it("moveKeyframe は外して置き直し、並びを保つ", () => {
    const op: Op = { op: "moveKeyframe", target, from: sec(0), keyframe: num(3, 4) };
    expect(applyKeyframeOp(p, op).keyframes?.map((k) => k.time.num)).toEqual([2, 3]);
  });

  it("removeKeyframe と setEase", () => {
    expect(applyKeyframeOp(p, { op: "removeKeyframe", target, time: sec(0) }).keyframes).toHaveLength(1);
    const eased = applyKeyframeOp(p, { op: "setEase", target, time: sec(0), interpolation: "hold" });
    expect(eased.keyframes?.[0]?.interpolation).toBe("hold");
  });
});
