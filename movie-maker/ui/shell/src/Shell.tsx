import { useEffect, useState } from "preact/hooks";
import {
  invoke,
  onDocChanged,
  type AnyPanel,
  type LayerSelection,
  type MediaListen,
  type Op,
  type PanelColumn,
  type PanelContext,
} from "@mm/sdk";
import { PANELS } from "./registry.ts";

/** 選択の変わったことを知らせるイベント。アプリの再生の操作列（`apps/movie-maker/web/transport.js`）が、Timeline の選択の Tauri イベントをこの名前の DOM のイベントにつなぐ。 */
export const LAYER_SELECTED_EVENT = "mm:layer-selected";

export type { LayerSelection };

export interface ShellProps {
  /** 操作を送る。本番は sdk の client（IPC）をつなぐ。拒否されたら理由の文字を返し、Shell が画面に出す。 */
  sendOp: (op: Op) => Promise<string | null> | undefined;
  /** パネルの id ごとの API。`createApi` を持つパネルは、ここに API があるときだけ出す。 */
  apis?: Record<string, unknown>;
  /** Tauri のイベントの購読。無ければ本番の listen。テストで差し替える。 */
  listen?: MediaListen;
  /** 出すパネルの候補。無ければ `panels.ts` の登録。テストで差し替える。 */
  panels?: readonly AnyPanel[];
}

const COLUMN_VIEW: Record<PanelColumn, { tabsClass: string; label: string; bodyClass: string; asideClass: string }> = {
  left: { tabsClass: "mm-shell-tabs", label: "Left panel", bodyClass: "mm-shell-left-body", asideClass: "mm-shell-left" },
  right: {
    tabsClass: "mm-shell-tabs mm-shell-right-tabs",
    label: "Right panel",
    bodyClass: "mm-shell-right-body",
    asideClass: "mm-shell-right",
  },
};

const Tabs = ({ column, panels, tab, onSelect }: { column: PanelColumn; panels: AnyPanel[]; tab: string; onSelect: (id: string) => void }) => (
  <div class={COLUMN_VIEW[column].tabsClass} role="tablist" aria-label={COLUMN_VIEW[column].label}>
    {panels.map(({ id, label }) => (
      <button key={id} type="button" role="tab" class="mm-shell-tab" aria-selected={tab === id} onClick={() => onSelect(id)}>
        {label}
      </button>
    ))}
  </div>
);

/** Timeline の選択のイベントを受けて、いま選ばれているレイヤーを持つ。 */
const useLayerSelection = (): LayerSelection => {
  const [selection, setSelection] = useState<LayerSelection>({ layer: null });
  useEffect(() => {
    const onSelected = (e: Event) => setSelection((e as CustomEvent<LayerSelection>).detail);
    window.addEventListener(LAYER_SELECTED_EVENT, onSelected);
    return () => window.removeEventListener(LAYER_SELECTED_EVENT, onSelected);
  }, []);
  return selection;
};

/** 操作を送り、拒否された理由を持つ。次に成功した操作で消える。 */
const useSendOp = (sendOp: ShellProps["sendOp"]) => {
  const [opError, setOpError] = useState<string | null>(null);
  const send = async (op: Op) => setOpError((await sendOp(op)) ?? null);
  return { send, opError };
};

/** `mm:doc-changed` が届いた回数。パネルの読み直しの合図にする。 */
const useDocChanges = (listen: MediaListen | undefined): number => {
  const [count, setCount] = useState(0);
  useEffect(() => {
    // 購読が確立する前に外れた場合も、確立した後で外す。
    let off: (() => void) | null = null;
    let stopped = false;
    // Tauri の外（デモや試験）では購読できないので、通知なしで動かす。
    onDocChanged(() => setCount((n) => n + 1), listen).then(
      (fn) => {
        if (stopped) fn();
        else off = fn;
      },
      () => undefined,
    );
    return () => {
      stopped = true;
      off?.();
    };
  }, [listen]);
  return count;
};

const bump = (counts: Record<string, number>, id: string): Record<string, number> => ({ ...counts, [id]: (counts[id] ?? 0) + 1 });

/** 1 つの列のタブの選択。最初は `order` の先頭のタブを見せる。 */
const useColumn = (panels: AnyPanel[]) => {
  const [selected, setSelected] = useState<string | null>(null);
  const [visits, setVisits] = useState<Record<string, number>>({});
  const [requests, setRequests] = useState<Record<string, number>>({});
  const tab = panels.find((p) => p.id === selected)?.id ?? panels[0]?.id ?? "";
  const select = (id: string) => {
    setVisits((v) => bump(v, id));
    setSelected(id);
  };
  const open = (id: string) => {
    setRequests((r) => bump(r, id));
    setSelected(id);
  };
  return { tab, select, open, visits, requests };
};

type ColumnState = ReturnType<typeof useColumn>;
type SharedContext = Pick<PanelContext<unknown>, "docChanges" | "selection" | "sendOp" | "runCommand">;

interface ColumnProps {
  name: PanelColumn;
  panels: AnyPanel[];
  state: ColumnState;
  apis: Record<string, unknown>;
  shared: SharedContext;
}

/** 1 つの列。タブ（2 つ以上のとき）と、全パネルの本体を作ったまま選んだものだけ見せる。 */
const Column = ({ name, panels, state, apis, shared }: ColumnProps) => (
  <>
    {panels.length > 1 && <Tabs column={name} panels={panels} tab={state.tab} onSelect={state.select} />}
    {panels.map((p) => (
      <div key={p.id} class={COLUMN_VIEW[name].bodyClass} role="tabpanel" hidden={state.tab !== p.id}>
        {p.render({
          ...shared,
          api: apis[p.id],
          visits: state.visits[p.id] ?? 0,
          requests: state.requests[p.id] ?? 0,
          active: state.tab === p.id,
        })}
      </div>
    ))}
  </>
);

/** 2 つの列。パネルは記述子の `column` の列に `order` の順でタブとして並ぶ。API の要るパネルは API があるときだけ出す。 */
export const Shell = ({ sendOp, apis = {}, listen, panels = PANELS }: ShellProps) => {
  const docChanges = useDocChanges(listen);
  const { send, opError } = useSendOp(sendOp);
  const selection = useLayerSelection();
  const shown = panels.filter((p) => !p.createApi || apis[p.id] !== undefined);
  const columns = {
    left: shown.filter((p) => p.column === "left"),
    right: shown.filter((p) => p.column === "right"),
  };
  const state = { left: useColumn(columns.left), right: useColumn(columns.right) };
  const runCommand = (command: string) => {
    const owner = shown.find((p) => p.commands?.includes(command));
    if (owner) state[owner.column].open(owner.id);
    else void invoke(command);
  };
  const shared = { docChanges, selection, sendOp: send, runCommand };
  return (
    <>
      <aside class={COLUMN_VIEW.left.asideClass}>
        <Column name="left" panels={columns.left} state={state.left} apis={apis} shared={shared} />
      </aside>
      <main class="mm-shell-center" />
      <aside class={COLUMN_VIEW.right.asideClass}>
        {opError && (
          <p class="mm-shell-error" role="alert">
            {opError}
          </p>
        )}
        <Column name="right" panels={columns.right} state={state.right} apis={apis} shared={shared} />
      </aside>
    </>
  );
};
