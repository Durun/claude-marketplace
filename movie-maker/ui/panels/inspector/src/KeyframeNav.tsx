import {
  type Interpolation,
  type Keyframe,
  type Op,
  type Param,
  type ParamRef,
  keyIndexAt,
  nextKeySec,
  prevKeySec,
} from "@mm/sdk";
import { activeKey, addKeyframeOp, interpolationOp, INTERPOLATIONS, removeKeyframeOp, type Playhead } from "./keyframes.ts";

export interface KeyframeNavProps {
  target: ParamRef;
  param: Param;
  at: Playhead;
  label: string;
  onOp: (op: Op) => void;
  /** 前後のキーフレームへ再生位置を動かす。無ければ前後の移動のボタンを出さない。 */
  onSeek?: (sec: number) => void;
}

const PREV = "◀";
const NEXT = "▶";
const KEY_ON = "◆";
const KEY_OFF = "◇";

interface SeekProps {
  to: number | null;
  glyph: string;
  label: string;
  onSeek: (sec: number) => void;
}

const SeekButton = ({ to, glyph, label, onSeek }: SeekProps) => (
  <button
    type="button"
    class="mm-insp-kfbtn"
    aria-label={label}
    disabled={to === null}
    onClick={() => to !== null && onSeek(to)}
  >
    {glyph}
  </button>
);

interface InterpProps {
  active: Keyframe | null;
  label: string;
  onPick: (key: Keyframe, i: Interpolation) => void;
}

const InterpSelect = ({ active, label, onPick }: InterpProps) => (
  <select
    class="mm-insp-kfinterp"
    aria-label={`${label} interpolation`}
    value={active?.interpolation ?? ""}
    disabled={!active}
    onChange={(e) => active && onPick(active, e.currentTarget.value as Interpolation)}
  >
    {!active && <option value="">-</option>}
    {INTERPOLATIONS.map((i) => (
      <option key={i} value={i}>
        {i}
      </option>
    ))}
  </select>
);

/**
 * AE のキーフレームナビゲーターの形（前へ、再生位置のキーの有無と切り替え、次へ）に、補間の切り替えを足したもの（キーフレームナビゲーター）。
 * 足す・外す・補間の切り替えはどれも 1 つの操作で、取り消し 1 回で戻る。
 */
export const KeyframeNav = ({ target, param, at, label, onOp, onSeek }: KeyframeNavProps) => {
  const hasKey = keyIndexAt(param, at.sec, at.fps) >= 0;
  const toggle = () => {
    const op = hasKey ? removeKeyframeOp(target, param, at) : addKeyframeOp(target, param, at);
    if (op) onOp(op);
  };
  return (
    <div class="mm-insp-kfnav">
      {onSeek && (
        <SeekButton to={prevKeySec(param, at.sec, at.fps)} glyph={PREV} label={`${label} previous keyframe`} onSeek={onSeek} />
      )}
      <button
        type="button"
        class="mm-insp-kfbtn mm-insp-kftoggle"
        aria-label={`${label} keyframe at playhead`}
        aria-pressed={hasKey}
        title={hasKey ? "Remove keyframe" : "Add keyframe"}
        onClick={toggle}
      >
        {hasKey ? KEY_ON : KEY_OFF}
      </button>
      {onSeek && (
        <SeekButton to={nextKeySec(param, at.sec, at.fps)} glyph={NEXT} label={`${label} next keyframe`} onSeek={onSeek} />
      )}
      <InterpSelect active={activeKey(param, at)} label={label} onPick={(k, i) => onOp(interpolationOp(target, k, i))} />
    </div>
  );
};
