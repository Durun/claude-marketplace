import type { FadeCurve, Layer, Op } from "@mm/sdk";
import {
  FADE_CURVES,
  FADE_CURVE_LABELS,
  addRectMaskOp,
  fadeSeconds,
  removeMaskOp,
  setFadeCurveOp,
  setFadeDurationOp,
  type FadeEdge,
} from "./maskOps.ts";

const FADE_STEP = 0.1;
const FADE_DIGITS = 3;
const FADE_EDGES: readonly { edge: FadeEdge; label: string }[] = [
  { edge: "fade_in", label: "fade in" },
  { edge: "fade_out", label: "fade out" },
];

/** 音の区画に並べる、フェードの長さ（秒）と曲線。 */
export const FadeRows = ({ layer, onOp }: { layer: Layer; onOp: (op: Op) => void }) => (
  <>
    {FADE_EDGES.map(({ edge, label }) => {
      const fade = layer.audio?.[edge];
      return (
        <div key={edge} class="mm-insp-row">
          <label class="mm-insp-label">{label}</label>
          <div class="mm-insp-value mm-insp-fade">
            <input
              type="number"
              class="mm-insp-input"
              aria-label={`${label} seconds`}
              min={0}
              step={FADE_STEP}
              value={Number(fadeSeconds(fade).toFixed(FADE_DIGITS))}
              onChange={(e) => {
                const n = e.currentTarget.valueAsNumber;
                if (Number.isFinite(n)) onOp(setFadeDurationOp(layer, edge, n));
              }}
            />
            <select
              class="mm-insp-input"
              aria-label={`${label} curve`}
              disabled={!fade}
              value={fade?.curve ?? "linear"}
              onChange={(e) => {
                const op = setFadeCurveOp(layer, edge, e.currentTarget.value as FadeCurve);
                if (op) onOp(op);
              }}
            >
              {FADE_CURVES.map((c) => (
                <option key={c} value={c}>
                  {FADE_CURVE_LABELS[c]}
                </option>
              ))}
            </select>
          </div>
        </div>
      );
    })}
  </>
);

/** マスクの区画の見出しに置く削除ボタン。 */
export const RemoveMaskButton = ({ maskId, onOp }: { maskId: string; onOp: (op: Op) => void }) => (
  <button
    type="button"
    class="mm-insp-kfbtn mm-insp-maskbtn"
    aria-label={`remove ${maskId}`}
    onClick={() => onOp(removeMaskOp(maskId))}
  >
    Remove
  </button>
);

/** 角丸の矩形のマスクを足すボタン。 */
export const AddMaskButton = ({ layer, onOp }: { layer: Layer; onOp: (op: Op) => void }) => (
  <button
    type="button"
    class="mm-insp-kfbtn mm-insp-maskbtn"
    aria-label="add rect mask"
    onClick={() => onOp(addRectMaskOp(layer))}
  >
    Add rounded rect
  </button>
);
