import type { RenderTarget } from "@mm/sdk";
import type { DeliverOverview } from "./api.ts";
import { extraFields, type FieldSpec } from "./form.ts";
import { formatDuration, formatFps } from "./format.ts";

/** 書き出す範囲の表示。sequence を選んでいればその全長、無ければレンダーターゲットのコンポジションの全体。 */
export const rangeLabel = (target: RenderTarget, overview: DeliverOverview, sequence: string): string => {
  const seq = overview.sequences.find((s) => s.id === sequence);
  if (seq) return `0:00 – ${formatDuration(seq.duration)} (sequence ${seq.id})`;
  const comp = overview.compositions.find((c) => c.id === target.composition);
  return comp ? `0:00 – ${formatDuration(comp.duration)} (${comp.id})` : `composition ${target.composition} is missing`;
};

export interface TargetRowProps {
  target: RenderTarget;
  specs: FieldSpec[];
  overview: DeliverOverview;
  sequence: string;
  /** キューに積んだ順番（1 から）。積んでいなければ `null`。 */
  position: number | null;
  onToggle: () => void;
  onEdit: () => void;
  onRemove: () => void;
}

/** レンダーターゲット 1 つの行。名前、大きさ、fps、コーデック、範囲、出力先。 */
export const TargetRow = ({ target, specs, overview, sequence, position, onToggle, onEdit, onRemove }: TargetRowProps) => (
  <li class="mm-deliver-row" data-queued={position !== null}>
    <label class="mm-deliver-check">
      <input type="checkbox" checked={position !== null} aria-label={`Queue ${target.id}`} onChange={onToggle} />
      {position !== null && <span class="mm-deliver-order">{position}</span>}
    </label>
    <div class="mm-deliver-row-body">
      <strong class="mm-deliver-name">{target.id}</strong>
      <div class="mm-deliver-meta">
        {target.width}×{target.height} · {formatFps(target.fps)} · {target.codec}
      </div>
      <div class="mm-deliver-meta">Range {rangeLabel(target, overview, sequence)}</div>
      <div class="mm-deliver-meta mm-deliver-path" title={target.output_path}>
        {target.output_path}
      </div>
      {extraFields(target, specs).map((f) => (
        <div class="mm-deliver-meta" key={f.label}>
          {f.label}: {f.text}
        </div>
      ))}
      <div class="mm-deliver-row-actions">
        <button type="button" class="mm-deliver-button" onClick={onEdit}>
          Edit
        </button>
        <button type="button" class="mm-deliver-button" onClick={onRemove}>
          Remove
        </button>
      </div>
    </div>
  </li>
);
