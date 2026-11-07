import {
  type Interpolation,
  type Keyframe,
  type Op,
  type Param,
  type ParamRef,
  type ParamValue,
  evalParam,
  frameTime,
  keyIndexAt,
  numbersOf,
  toSec,
} from "@mm/sdk";

/** 再生位置とフレームの数え方。キーフレームの時刻はフレームの先頭にそろえる。 */
export interface Playhead {
  /** コンポジションの秒。 */
  sec: number;
  fps: number;
}

/** 補間の切り替えの並び（Inspector の選択肢の順）。 */
export const INTERPOLATIONS: readonly Interpolation[] = ["linear", "bezier", "hold"];

/** 補間できる値か。数、2・3 次元の数、色。それ以外はホールドだけ（mm-eval）。 */
const interpolable = (v: ParamValue): boolean => numbersOf(v) !== null || v.type === "color";

/**
 * 再生位置の区間の形を決めるキーフレーム（位置にあればそれ、無ければ直前）。
 * 補間の切り替えはこのキーフレームに掛ける（区間の形は始点で決まるため）。
 */
export const activeKey = (param: Param, at: Playhead): Keyframe | null => {
  const keys = param.keyframes ?? [];
  const here = keys[keyIndexAt(param, at.sec, at.fps)];
  if (here) return here;
  const frame = toSec(frameTime(at.sec, at.fps));
  return keys.findLast((k) => toSec(k.time) < frame) ?? null;
};

/**
 * 再生位置にキーフレームを足す操作。値は今の評価値（AE のキーフレームナビゲーターと同じ）。
 * 補間は直前のキーフレームを引き継ぎ、無ければ補間できる値は linear、それ以外は hold。
 * ease 無しの linear の区間は形が変わらない。ベジェとイージングの区間は、制御点を区間ごとに持つので形が少し変わる。
 */
export const addKeyframeOp = (target: ParamRef, param: Param, at: Playhead): Op => {
  // 値は置く先のフレームの先頭で評価する。再生位置のままだと、フレームの途中の値がずれて曲線が変わる。
  const time = frameTime(at.sec, at.fps);
  const value = evalParam(param, toSec(time));
  const prev = activeKey(param, at);
  const interpolation: Interpolation = prev?.interpolation ?? (interpolable(value) ? "linear" : "hold");
  return {
    op: "setKeyframe",
    target,
    keyframe: { time, value, interpolation, ease: prev?.ease, bezier: prev?.bezier },
  };
};

/** 再生位置のキーフレームを外す操作。無ければ null。 */
export const removeKeyframeOp = (target: ParamRef, param: Param, at: Playhead): Op | null => {
  const k = param.keyframes?.[keyIndexAt(param, at.sec, at.fps)];
  return k ? { op: "removeKeyframe", target, time: k.time } : null;
};

/** キーフレームの補間を変える操作。イージングとベジェの制御点は残す。 */
export const interpolationOp = (target: ParamRef, key: Keyframe, interpolation: Interpolation): Op => ({
  op: "setEase",
  target,
  time: key.time,
  interpolation,
  ease: key.ease,
  bezier: key.bezier,
});

/**
 * キーフレームのあるパラメータの値を入力欄で変えたときの操作。
 * 再生位置のキーフレームの値を変え（無ければ足し）、区間の形は残す（AE と同じく、動きのある値は時刻つきで変える）。
 */
export const valueAtOp = (target: ParamRef, param: Param, at: Playhead, value: ParamValue): Op => {
  const here = param.keyframes?.[keyIndexAt(param, at.sec, at.fps)];
  if (here) return { op: "setKeyframe", target, keyframe: { ...here, value } };
  const added = addKeyframeOp(target, param, at);
  return added.op === "setKeyframe" ? { ...added, keyframe: { ...added.keyframe, value } } : added;
};
