import { useMemo, useState } from "preact/hooks";
import type { RenderTarget } from "@mm/sdk";
import type { DeliverApi, DeliverOverview } from "./api.ts";
import { ExportQueue } from "./ExportQueue.tsx";
import { fieldSpecs, type FieldSpec } from "./form.ts";
import { formatDuration } from "./format.ts";
import { customPreset, defaultComposition, PRESETS, targetFromPreset, type Preset } from "./presets.ts";
import { TargetForm } from "./TargetForm.tsx";
import { TargetRow } from "./TargetRow.tsx";
import { replaceTargetOps, useDeliver, type DeliverState } from "./useDeliver.ts";

export interface DeliverPanelProps {
  api: DeliverApi;
  /** 増えるたびにレンダーターゲットの一覧を読み直す（外から編集されたとき）。 */
  refreshKey?: number;
}

/** いま開いている編集。`original` が無ければ足す前の初期値。 */
interface Editing {
  target: RenderTarget;
  original: string | null;
}

const PresetBar = ({ onAdd }: { onAdd: (preset: Preset) => void }) => {
  const [id, setId] = useState(PRESETS[0]?.id ?? "");
  const all = [...PRESETS, customPreset];
  return (
    <div class="mm-deliver-toolbar">
      <select aria-label="Preset" value={id} onChange={(e) => setId((e.target as HTMLSelectElement).value)}>
        {all.map((p) => (
          <option key={p.id} value={p.id}>
            {p.label}
          </option>
        ))}
      </select>
      <button type="button" class="mm-deliver-button" onClick={() => onAdd(all.find((p) => p.id === id) ?? customPreset)}>
        Add target
      </button>
    </div>
  );
};

const Options = ({ overview, deliver }: { overview: DeliverOverview; deliver: DeliverState }) => (
  <div class="mm-deliver-options">
    <div class="mm-deliver-field">
      <label for="mm-deliver-sequence">Range</label>
      <select id="mm-deliver-sequence" value={deliver.sequence} onChange={(e) => deliver.setSequence((e.target as HTMLSelectElement).value)}>
        <option value="">Whole composition</option>
        {overview.sequences.map((s) => (
          <option key={s.id} value={s.id}>
            Sequence {s.id} ({formatDuration(s.duration)})
          </option>
        ))}
      </select>
    </div>
    <label class="mm-deliver-check-line">
      <input type="checkbox" checked={deliver.verify} disabled={!overview.can_verify} onChange={(e) => deliver.setVerify((e.target as HTMLInputElement).checked)} />
      Verify output with ffprobe
      {!overview.can_verify && <span class="mm-deliver-meta"> (not available in this build)</span>}
    </label>
  </div>
);

const StartBar = ({ deliver }: { deliver: DeliverState }) => {
  const n = deliver.queue.length;
  return (
    <div class="mm-deliver-toolbar">
      <button type="button" class="mm-deliver-button mm-deliver-primary" disabled={n === 0 || deliver.status.running || deliver.starting} onClick={deliver.start}>
        {n === 0 ? "Export" : `Export ${n} ${n === 1 ? "target" : "targets"}`}
      </button>
      {n === 0 && <span class="mm-deliver-hint">Check the targets to export.</span>}
    </div>
  );
};

const useEditing = (deliver: DeliverState, specs: FieldSpec[]) => {
  const [editing, setEditing] = useState<Editing | null>(null);
  const overview = deliver.overview;
  const add = (preset: Preset) => {
    if (!overview) return;
    const composition = defaultComposition(preset, overview);
    const takenIds = overview.targets.map((t) => t.id);
    setEditing({ target: targetFromPreset(preset, { composition, takenIds }, specs), original: null });
  };
  const submit = async (value: RenderTarget): Promise<boolean> => {
    const original = editing?.original ?? null;
    const index = overview?.targets.findIndex((t) => t.id === original) ?? -1;
    const ops = original === null || index < 0 ? [{ op: "addRenderTarget" as const, target: value }] : replaceTargetOps(original, value, index);
    const ok = await deliver.applyOps(ops);
    if (ok) setEditing(null);
    return ok;
  };
  const remove = (id: string) => void deliver.applyOps([{ op: "removeRenderTarget", target: id }]);
  return { editing, setEditing, add, submit, remove };
};

/** Deliver。レンダーターゲットの一覧と編集、書き出しのキュー。入力欄はスキーマから作る。 */
export const DeliverPanel = ({ api, refreshKey = 0 }: DeliverPanelProps) => {
  const deliver = useDeliver(api, refreshKey);
  const specs = useMemo(() => fieldSpecs(), []);
  const { editing, setEditing, add, submit, remove } = useEditing(deliver, specs);
  const { overview } = deliver;
  return (
    <section class="mm-deliver" aria-label="Deliver">
      <PresetBar onAdd={add} />
      {deliver.error && (
        <p class="mm-deliver-error" role="alert">
          {deliver.error}
        </p>
      )}
      {overview && editing && editing.original === null && (
        <TargetForm key="new" target={editing.target} specs={specs} overview={overview} api={api} submitLabel="Add" onSubmit={submit} onCancel={() => setEditing(null)} />
      )}
      <ul class="mm-deliver-targets" aria-label="Render targets">
        {overview?.targets.map((t) =>
          editing?.original === t.id ? (
            <li key={t.id}>
              <TargetForm target={t} specs={specs} overview={overview} api={api} submitLabel="Save" onSubmit={submit} onCancel={() => setEditing(null)} />
            </li>
          ) : (
            <TargetRow
              key={t.id}
              target={t}
              specs={specs}
              overview={overview}
              sequence={deliver.sequence}
              position={deliver.queue.includes(t.id) ? deliver.queue.indexOf(t.id) + 1 : null}
              onToggle={() => deliver.toggleQueue(t.id)}
              onEdit={() => setEditing({ target: t, original: t.id })}
              onRemove={() => remove(t.id)}
            />
          ),
        )}
        {overview?.targets.length === 0 && <li class="mm-deliver-empty">No render targets yet. Pick a preset and add one.</li>}
      </ul>
      {overview && <Options overview={overview} deliver={deliver} />}
      <StartBar deliver={deliver} />
      <ExportQueue status={deliver.status} onCancel={deliver.cancel} onReveal={deliver.reveal} />
    </section>
  );
};
