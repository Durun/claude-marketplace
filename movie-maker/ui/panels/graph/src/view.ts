import {
  type CubicBezier,
  type Keyframe,
  type Param,
  EASE_IN_OUT,
  frameTime,
  numbersOf,
  segmentProgress,
  toSec,
  withNumbers,
} from "@mm/sdk";

/** 描く範囲（秒と値）と、描く先の大きさ（px）。 */
export interface View {
  t0: number;
  t1: number;
  v0: number;
  v1: number;
  width: number;
  height: number;
}

/** 曲線の周りに空ける割合。端のキーフレームとベジェのハンドルが枠に掛からないようにする。 */
const MARGIN = 0.1;
/** キーフレームが 1 つか値が一定のときに見せる幅。 */
const MIN_SPAN_SEC = 1;
const MIN_SPAN_VALUE = 1;
/** ベジェの区間を折れ線にする点の数。 */
const BEZIER_STEPS = 32;
const HALF = 0.5;

/** ベジェのハンドルの側。out は区間の始点から出る側（x1, y1）、in は終点へ入る側（x2, y2）。 */
export type Side = "out" | "in";
export const SIDES: readonly Side[] = ["out", "in"];

/** 秒を横の px にする。 */
export const xOf = (v: View, sec: number): number => ((sec - v.t0) / (v.t1 - v.t0)) * v.width;
/** 値を縦の px にする（上が大きい値）。 */
export const yOf = (v: View, value: number): number => v.height - ((value - v.v0) / (v.v1 - v.v0)) * v.height;
/** 横の px を秒にする。 */
export const secOf = (v: View, x: number): number => v.t0 + (x / v.width) * (v.t1 - v.t0);
/** 縦の px を値にする。 */
export const valueOf = (v: View, y: number): number => v.v0 + ((v.height - y) / v.height) * (v.v1 - v.v0);

/** 成分 c のキーフレームの値。 */
export const keyValue = (k: Keyframe, c: number): number => numbersOf(k.value)?.[c] ?? 0;

const padded = (lo: number, hi: number, min: number): [number, number] => {
  const mid = (lo + hi) * HALF;
  const span = Math.max(hi - lo, min);
  const pad = span * MARGIN;
  return [mid - span * HALF - pad, mid + span * HALF + pad];
};

/** 区間のベジェの制御点（`bezier` の区間でなければ null）。 */
export const segmentBezier = (k: Keyframe): CubicBezier | null =>
  k.interpolation === "bezier" ? (k.bezier ?? EASE_IN_OUT) : null;

/** ベジェのハンドルの位置（秒と値）。区間 i の始点側が 1、終点側が 2。 */
export const handlePoint = (from: Keyframe, to: Keyframe, side: Side, c: number): [number, number] => {
  const b = segmentBezier(from) ?? EASE_IN_OUT;
  const [t0, t1] = [toSec(from.time), toSec(to.time)];
  const [v0, v1] = [keyValue(from, c), keyValue(to, c)];
  const [x, y] = side === "out" ? [b.x1, b.y1] : [b.x2, b.y2];
  return [t0 + x * (t1 - t0), v0 + y * (v1 - v0)];
};

/** 全部のキーフレームとベジェのハンドルが収まる範囲（Blender の View All、AE の Fit All に当たる）。 */
export const fitView = (param: Param, components: number, width: number, height: number): View => {
  const keys = param.keyframes ?? [];
  const times = keys.map((k) => toSec(k.time));
  const values: number[] = [];
  keys.forEach((k, i) => {
    const next = keys[i + 1];
    for (let c = 0; c < components; c++) {
      values.push(keyValue(k, c));
      if (next && segmentBezier(k)) values.push(...SIDES.map((s) => handlePoint(k, next, s, c)[1]));
    }
  });
  if (values.length === 0) values.push(...(numbersOf(param.value) ?? [0]));
  const [t0, t1] = padded(Math.min(...times, 0), Math.max(...times, 0), MIN_SPAN_SEC);
  const [v0, v1] = padded(Math.min(...values), Math.max(...values), MIN_SPAN_VALUE);
  return { t0, t1, v0, v1, width, height };
};

/** path に書く px の桁。1/100 px より細かい差は見えない。 */
const PATH_DIGITS = 2;

const point = (v: View, sec: number, value: number): string =>
  `${xOf(v, sec).toFixed(PATH_DIGITS)},${yOf(v, value).toFixed(PATH_DIGITS)}`;

const segmentPoints = (v: View, from: Keyframe, to: Keyframe, c: number): string[] => {
  const [t0, t1] = [toSec(from.time), toSec(to.time)];
  const [v0, v1] = [keyValue(from, c), keyValue(to, c)];
  if (from.interpolation === "hold") return [point(v, t1, v0), point(v, t1, v1)];
  const steps = from.interpolation === "linear" && (from.ease ?? "none") === "none" ? 1 : BEZIER_STEPS;
  return Array.from({ length: steps }, (_, i) => {
    const sec = t0 + ((i + 1) / steps) * (t1 - t0);
    return point(v, sec, v0 + (v1 - v0) * segmentProgress(from, to, sec));
  });
};

/** 成分 c の値の曲線の SVG の path。最初より前と最後より後は水平に伸ばす（mm-eval と同じ）。 */
export const curvePath = (param: Param, c: number, v: View): string => {
  const keys = param.keyframes ?? [];
  const first = keys[0];
  if (!first) {
    const value = numbersOf(param.value)?.[c] ?? 0;
    return `M${point(v, v.t0, value)}L${point(v, v.t1, value)}`;
  }
  const pts = [point(v, v.t0, keyValue(first, c)), point(v, toSec(first.time), keyValue(first, c))];
  keys.forEach((k, i) => {
    const next = keys[i + 1];
    if (next) pts.push(...segmentPoints(v, k, next, c));
  });
  const last = keys[keys.length - 1] ?? first;
  pts.push(point(v, v.t1, keyValue(last, c)));
  return `M${pts.join("L")}`;
};

/** ドラッグの動き。秒と値の差、フレームの fps、Shift で 1 つの向きに絞るか。 */
export interface KeyDrag {
  dSec: number;
  dValue: number;
  fps: number;
  /** Shift。横と縦のうち、px で大きく動いた向きだけを使う。 */
  constrain?: { horizontal: boolean };
}

const clampTime = (keys: readonly Keyframe[], i: number, sec: number, fps: number): number => {
  const frame = 1 / fps;
  const prev = keys[i - 1];
  const next = keys[i + 1];
  const lo = prev ? toSec(prev.time) + frame : -Infinity;
  const hi = next ? toSec(next.time) - frame : Infinity;
  return Math.min(hi, Math.max(lo, sec));
};

/**
 * キーフレーム i の成分 c をドラッグした後のキーフレーム。
 * 時刻はフレームにそろえ、隣のキーフレームを越えない（並びが変わらず、同じ時刻に重ならない）。
 */
export const dragKey = (keys: readonly Keyframe[], i: number, c: number, drag: KeyDrag): Keyframe | null => {
  const k = keys[i];
  const nums = k && numbersOf(k.value);
  if (!k || !nums) return null;
  const useTime = drag.constrain?.horizontal ?? true;
  const useValue = !drag.constrain || !drag.constrain.horizontal;
  const sec = clampTime(keys, i, toSec(k.time) + (useTime ? drag.dSec : 0), drag.fps);
  const value = (nums[c] ?? 0) + (useValue ? drag.dValue : 0);
  return {
    ...k,
    time: frameTime(sec, drag.fps),
    value: withNumbers(k.value, nums.map((x, j) => (j === c ? value : x))),
  };
};

const EPSILON = 1e-9;

/**
 * 区間 i のベジェのハンドルを (sec, value) へ動かした後の制御点。
 * x は [0,1] に収めて時間を単調に保つ（AE と Blender のハンドルが区間の外へ出ないのと同じ）。
 * 区間の両端の値が同じときは y を決められないので、y は元のまま。
 */
export const dragHandle = (
  keys: readonly Keyframe[],
  i: number,
  side: Side,
  c: number,
  at: { sec: number; value: number },
): CubicBezier | null => {
  const from = keys[i];
  const to = keys[i + 1];
  if (!from || !to) return null;
  const b = segmentBezier(from) ?? EASE_IN_OUT;
  const [t0, t1] = [toSec(from.time), toSec(to.time)];
  const [v0, v1] = [keyValue(from, c), keyValue(to, c)];
  const x = Math.min(1, Math.max(0, (at.sec - t0) / (t1 - t0)));
  const dv = v1 - v0;
  const old = side === "out" ? b.y1 : b.y2;
  const y = Math.abs(dv) < EPSILON ? old : (at.value - v0) / dv;
  return side === "out" ? { ...b, x1: x, y1: y } : { ...b, x2: x, y2: y };
};
