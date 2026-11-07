import type { CubicBezier, Ease, Keyframe, Op, Param, ParamValue, RationalTime } from "./generated/ops.ts";

/** `Bezier` の制御点が無いときの既定（mm-doc の `CubicBezier::EASE_IN_OUT`）。 */
export const EASE_IN_OUT: CubicBezier = { x1: 0.42, y1: 0, x2: 0.58, y2: 1 };

const NEWTON_ITERATIONS = 8;
const BISECTION_ITERATIONS = 40;
const BEZIER_EPSILON = 1e-7;
const HALF = 0.5;
const TWO = 2;
const THREE = 3;
const SIX = 6;
/** NTSC 系の fps（29.97 など）を有理数にするときの分母。 */
const NTSC_DEN = 1001;
const FPS_TOLERANCE = 1e-6;

/** 有理数の時刻を秒にする。 */
export const toSec = (t: RationalTime): number => t.num / t.den;

/** 2 つの時刻が同じか。分母が違っても値で比べる。 */
export const sameTime = (a: RationalTime, b: RationalTime): boolean => a.num * b.den === b.num * a.den;

const gcd = (a: number, b: number): number => (b === 0 ? Math.abs(a) : gcd(b, a % b));

/** fps を有理数（num/den）にする。整数か NTSC 系（x/1001）だけを正確に扱う。 */
export const fpsRational = (fps: number): { num: number; den: number } => {
  if (Math.abs(fps - Math.round(fps)) < FPS_TOLERANCE) return { num: Math.round(fps), den: 1 };
  return { num: Math.round(fps * NTSC_DEN), den: NTSC_DEN };
};

/** 秒を、いちばん近いフレームの先頭の時刻（既約の有理数）にする。キーフレームの時刻はフレームにそろえる。 */
export const frameTime = (sec: number, fps: number): RationalTime => {
  const r = fpsRational(fps);
  const frame = Math.round((sec * r.num) / r.den);
  const num = frame * r.den;
  const g = gcd(num, r.num) || 1;
  return { num: num / g, den: r.num / g };
};

const bezier1d = (c1: number, c2: number, s: number): number => {
  const u = 1 - s;
  return THREE * u * u * s * c1 + THREE * u * s * s * c2 + s * s * s;
};

const bezier1dDerivative = (c1: number, c2: number, s: number): number => {
  const u = 1 - s;
  return THREE * u * u * c1 + SIX * u * s * (c2 - c1) + THREE * s * s * (1 - c2);
};

const solveX = (b: CubicBezier, x: number): number => {
  let s = x;
  for (let i = 0; i < NEWTON_ITERATIONS; i++) {
    const dx = bezier1d(b.x1, b.x2, s) - x;
    if (Math.abs(dx) < BEZIER_EPSILON) return s;
    const d = bezier1dDerivative(b.x1, b.x2, s);
    if (Math.abs(d) < BEZIER_EPSILON) break;
    s -= dx / d;
  }
  let [lo, hi] = [0, 1];
  for (let i = 0; i < BISECTION_ITERATIONS; i++) {
    s = (lo + hi) / TWO;
    if (bezier1d(b.x1, b.x2, s) < x) lo = s;
    else hi = s;
  }
  return s;
};

/** CSS の `cubic-bezier` と同じ曲線の、進み具合 x での y（mm-eval の `y_at`）。 */
export const bezierY = (b: CubicBezier, x: number): number => {
  if (x <= 0) return 0;
  if (x >= 1) return 1;
  return bezier1d(b.y1, b.y2, solveX(b, x));
};

/** `Linear` の進み具合に掛けるイージング（mm-eval の `ease`）。 */
export const easeProgress = (kind: Ease | undefined, p: number): number => {
  switch (kind) {
    case "in":
      return p * p;
    case "out":
      return 1 - (1 - p) * (1 - p);
    case "in_out":
      return p < HALF ? TWO * p * p : 1 - TWO * (1 - p) * (1 - p);
    default:
      return p;
  }
};

/** 区間 `[from, to)` の中の時刻 sec での、補間を掛けた進み具合。`hold` は 0。 */
export const segmentProgress = (from: Keyframe, to: Keyframe, sec: number): number => {
  const span = toSec(to.time) - toSec(from.time);
  const p = span <= 0 ? 1 : Math.min(1, Math.max(0, (sec - toSec(from.time)) / span));
  if (from.interpolation === "hold") return 0;
  if (from.interpolation === "linear") return easeProgress(from.ease, p);
  return bezierY(from.bezier ?? EASE_IN_OUT, p);
};

/** 曲線に描ける成分。数は 1 つ、2・3 次元の数は成分ごと。それ以外は null。 */
export const numbersOf = (v: ParamValue): number[] | null => {
  if (v.type === "number") return [v.value];
  if (v.type === "vec2" || v.type === "vec3") return v.value;
  return null;
};

/** 成分の並びを元の値と同じ種類に戻す。 */
export const withNumbers = (v: ParamValue, nums: readonly number[]): ParamValue => {
  if (v.type === "number") return { type: "number", value: nums[0] ?? v.value };
  if (v.type === "vec2" || v.type === "vec3") return { type: v.type, value: v.value.map((x, i) => nums[i] ?? x) };
  return v;
};

const lerp = (a: number, b: number, p: number): number => a + (b - a) * p;

const lerpValue = (a: ParamValue, b: ParamValue, p: number): ParamValue => {
  if (a.type === "color" && b.type === "color") {
    const [x, y] = [a.value, b.value];
    return { type: "color", value: { r: lerp(x.r, y.r, p), g: lerp(x.g, y.g, p), b: lerp(x.b, y.b, p), a: lerp(x.a, y.a, p) } };
  }
  const [na, nb] = [numbersOf(a), numbersOf(b)];
  if (!na || !nb || a.type !== b.type) return a;
  return withNumbers(a, na.map((x, i) => lerp(x, nb[i] ?? x, p)));
};

/** 時刻 sec でのパラメータの値（mm-eval の `eval_param` と同じ決まり）。 */
export const evalParam = (param: Param, sec: number): ParamValue => {
  const keys = param.keyframes ?? [];
  const first = keys[0];
  if (!first) return param.value;
  if (sec <= toSec(first.time)) return first.value;
  const i = keys.findLastIndex((k) => toSec(k.time) <= sec);
  const from = keys[i] ?? first;
  const to = keys[i + 1];
  if (!to) return from.value;
  if (from.interpolation === "hold") return from.value;
  return lerpValue(from.value, to.value, segmentProgress(from, to, sec));
};

/** 時刻 sec のフレームにあるキーフレームの番号。無ければ -1。 */
export const keyIndexAt = (param: Param, sec: number, fps: number): number => {
  const at = frameTime(sec, fps);
  return (param.keyframes ?? []).findIndex((k) => sameTime(k.time, at));
};

/** sec より前（フレームで比べる）の最後のキーフレームの時刻（秒）。無ければ null。 */
export const prevKeySec = (param: Param, sec: number, fps: number): number | null => {
  const at = toSec(frameTime(sec, fps));
  const found = (param.keyframes ?? []).map((k) => toSec(k.time)).filter((t) => t < at);
  return found.length > 0 ? Math.max(...found) : null;
};

/** sec より後（フレームで比べる）の最初のキーフレームの時刻（秒）。無ければ null。 */
export const nextKeySec = (param: Param, sec: number, fps: number): number | null => {
  const at = toSec(frameTime(sec, fps));
  const found = (param.keyframes ?? []).map((k) => toSec(k.time)).filter((t) => t > at);
  return found.length > 0 ? Math.min(...found) : null;
};

const sorted = (keys: Keyframe[]): Keyframe[] => [...keys].sort((a, b) => toSec(a.time) - toSec(b.time));

const putKey = (keys: Keyframe[], k: Keyframe): Keyframe[] =>
  sorted([...keys.filter((x) => !sameTime(x.time, k.time)), k]);

/**
 * キーフレームの操作をパラメータに適用した結果（mm-ops と同じ決まり）。
 * ドラッグ中の見た目とデモが使う。プロジェクトのデータを変えるのは操作を送った先だけ。
 */
export const applyKeyframeOp = (param: Param, op: Op): Param => {
  const keys = param.keyframes ?? [];
  switch (op.op) {
    case "setKeyframe":
      return { ...param, keyframes: putKey(keys, op.keyframe) };
    case "removeKeyframe":
      return { ...param, keyframes: keys.filter((k) => !sameTime(k.time, op.time)) };
    case "moveKeyframe":
      return { ...param, keyframes: putKey(keys.filter((k) => !sameTime(k.time, op.from)), op.keyframe) };
    case "setEase":
      return {
        ...param,
        keyframes: keys.map((k) =>
          sameTime(k.time, op.time) ? { ...k, interpolation: op.interpolation, ease: op.ease, bezier: op.bezier } : k,
        ),
      };
    default:
      return param;
  }
};
