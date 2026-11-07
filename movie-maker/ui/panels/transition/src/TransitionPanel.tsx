import { useCallback, useEffect, useState } from "preact/hooks";
import type { CutTransition, TransitionApi, TransitionCut, TransitionState } from "./api.ts";

/** 置くときの長さ（秒）。パックのマクロ `transition` の既定と同じ。 */
export const DEFAULT_DURATION = 0.7;
const DURATION_STEP = 0.05;
const DURATION_MIN = 0.1;
const NONE = "";
const CUSTOM = "\u0000custom";

export interface TransitionPanelProps {
  api: TransitionApi;
  /** 増えるたびに読み直す（タブを開いたとき、外から編集されたとき）。 */
  refreshKey?: number;
}

const message = (e: unknown): string => (e instanceof Error ? e.message : String(e));

const DURATION_DIGITS = 2;
const label = (t: CutTransition): string => `${t.kind ?? "custom"} ${t.duration.toFixed(DURATION_DIGITS)}s`;

/** 読み込みと、操作の送信（終わるたびに読み直す）。 */
const useTransitions = (api: TransitionApi, refreshKey: number) => {
  const [state, setState] = useState<TransitionState | null>(null);
  const [kinds, setKinds] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [next, names] = await Promise.all([api.cuts(), api.kinds()]);
      setState(next);
      setKinds(names);
    } catch (e) {
      setError(message(e));
    }
  }, [api]);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  const run = useCallback(
    async (send: () => Promise<void>) => {
      try {
        await send();
        setError(null);
      } catch (e) {
        setError(message(e));
      }
      await load();
    },
    [load],
  );
  return { state, kinds, error, run };
};

/** カットを幅（長さに比例）で並べた帯。境のトランジションは、カットの頭に印と名前を出す。 */
const Strip = ({ cuts, selected, onSelect }: { cuts: TransitionCut[]; selected: string | null; onSelect: (id: string) => void }) => (
  <ol class="mm-tr-strip" aria-label="Cuts">
    {cuts.map((cut) => (
      <li key={cut.id} class="mm-tr-cell" style={{ flexGrow: cut.duration }}>
        <button
          type="button"
          class="mm-tr-cut"
          data-selected={selected === cut.id}
          aria-pressed={selected === cut.id}
          title={cut.transition ? `${cut.id}: ${label(cut.transition)}` : cut.id}
          onClick={() => onSelect(cut.id)}
        >
          {cut.transition && <span class="mm-tr-mark" role="img" aria-label={`Transition ${label(cut.transition)}`} />}
          <span class="mm-tr-cut-id">{cut.id}</span>
          {cut.transition && <span class="mm-tr-cut-kind">{cut.transition.kind ?? "custom"}</span>}
        </button>
      </li>
    ))}
  </ol>
);

interface EditorProps {
  cut: TransitionCut;
  first: boolean;
  kinds: string[];
  onSet: (kind: string, duration: number) => void;
  onRemove: () => void;
}

/** 選んだカットの頭のトランジション。種類を選ぶと置く・差し替える、「なし」で外す。長さは確定したときに 1 回。 */
const Editor = ({ cut, first, kinds, onSet, onRemove }: EditorProps) => {
  const tr = cut.transition;
  const value = tr ? (tr.kind ?? CUSTOM) : NONE;
  const duration = tr?.duration ?? DEFAULT_DURATION;
  const pick = (next: string) => {
    if (next === NONE) onRemove();
    else if (next !== CUSTOM) onSet(next, duration);
  };
  const resize = (text: string) => {
    const n = Number(text);
    if (tr?.kind && Number.isFinite(n) && n > 0 && n !== tr.duration) onSet(tr.kind, n);
  };
  return (
    <div class="mm-tr-editor">
      <h3 class="mm-tr-title">Transition into {cut.id}</h3>
      {first ? (
        <p class="mm-tr-meta">The first cut has no previous cut.</p>
      ) : (
        <>
          <label class="mm-tr-row">
            <span>Kind</span>
            <select class="mm-tr-input" aria-label="Transition kind" value={value} onChange={(e) => pick(e.currentTarget.value)}>
              <option value={NONE}>None</option>
              {tr && tr.kind === null && <option value={CUSTOM}>custom</option>}
              {kinds.map((k) => (
                <option key={k} value={k}>
                  {k}
                </option>
              ))}
            </select>
          </label>
          <label class="mm-tr-row">
            <span>Length (s)</span>
            <input
              key={`${cut.id}:${duration}`}
              class="mm-tr-input"
              type="number"
              aria-label="Transition length"
              min={DURATION_MIN}
              step={DURATION_STEP}
              defaultValue={duration}
              disabled={!tr?.kind}
              onChange={(e) => resize(e.currentTarget.value)}
            />
          </label>
        </>
      )}
    </div>
  );
};

/** シーケンスのカットの境のトランジションを見せて、種類と長さを選ぶ・外す。 */
export const TransitionPanel = ({ api, refreshKey = 0 }: TransitionPanelProps) => {
  const { state, kinds, error, run } = useTransitions(api, refreshKey);
  const [picked, setPicked] = useState<string | null>(null);
  const cuts = state?.cuts ?? [];
  const index = cuts.findIndex((c) => c.id === picked);
  const current = cuts[index >= 0 ? index : 0];
  return (
    <section class="mm-tr" aria-label="Transitions">
      {error && <p class="mm-tr-error">{error}</p>}
      {state && state.sequence === null && <p class="mm-tr-meta">No sequence in the open composition.</p>}
      {cuts.length > 0 && <Strip cuts={cuts} selected={current?.id ?? null} onSelect={setPicked} />}
      {current && (
        <Editor
          cut={current}
          first={current === cuts[0]}
          kinds={kinds}
          onSet={(kind, duration) => void run(() => api.set(current.id, kind, duration))}
          onRemove={() => void run(() => api.remove(current.id))}
        />
      )}
    </section>
  );
};
