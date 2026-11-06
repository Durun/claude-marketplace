import { useEffect, useState } from "preact/hooks";
import { InspectorPanel } from "@mm/inspector";
import { MediaPanel } from "@mm/media";
import { invoke, type Asset, type Layer, type MediaApi, type Op } from "@mm/sdk";
import { TasksPanel } from "@mm/tasks";

/** 選択の変わったことを知らせるイベント。アプリの再生の操作列（`apps/movie-maker/web/transport.js`）が、Timeline の選択の Tauri イベントをこの名前の DOM のイベントにつなぐ。 */
export const LAYER_SELECTED_EVENT = "mm:layer-selected";

export interface LayerSelection {
  layer: Layer | null;
  assets?: Asset[];
}

export interface ShellProps {
  /** 操作を送る。本番は sdk の client（IPC）をつなぐ。 */
  sendOp: (op: Op) => void;
  /** Media パネルがアプリに頼む窓口。無ければ Media は出さない。 */
  media?: MediaApi;
}

/** 左の列で見せているパネル。XP のムービーメーカーの Tasks と Collections の切り替えに倣う。 */
type LeftTab = "tasks" | "media";

/** Tasks の項目のうち、Media を開いて取り込みを始めるもの。 */
const IMPORT_COMMAND = "media.import";

const TABS: { id: LeftTab; label: string }[] = [
  { id: "tasks", label: "Tasks" },
  { id: "media", label: "Media" },
];

const LeftTabs = ({ tab, onSelect }: { tab: LeftTab; onSelect: (tab: LeftTab) => void }) => (
  <div class="mm-shell-tabs" role="tablist" aria-label="Left panel">
    {TABS.map(({ id, label }) => (
      <button
        key={id}
        type="button"
        role="tab"
        class="mm-shell-tab"
        aria-selected={tab === id}
        onClick={() => onSelect(id)}
      >
        {label}
      </button>
    ))}
  </div>
);

/** 左に Tasks と Media（タブで切り替え）、右に Inspector。 */
export const Shell = ({ sendOp, media }: ShellProps) => {
  const [selection, setSelection] = useState<LayerSelection>({ layer: null });
  const [tab, setTab] = useState<LeftTab>("tasks");
  const [importRequest, setImportRequest] = useState(0);
  const onTaskInvoke = (command: string) => {
    if (media && command === IMPORT_COMMAND) {
      setTab("media");
      setImportRequest((n) => n + 1);
      return;
    }
    void invoke(command);
  };
  useEffect(() => {
    const onSelected = (e: Event) => setSelection((e as CustomEvent<LayerSelection>).detail);
    window.addEventListener(LAYER_SELECTED_EVENT, onSelected);
    return () => window.removeEventListener(LAYER_SELECTED_EVENT, onSelected);
  }, []);
  return (
    <>
      <aside class="mm-shell-left">
        {media && <LeftTabs tab={tab} onSelect={setTab} />}
        <div class="mm-shell-left-body" role="tabpanel" hidden={tab !== "tasks"}>
          <TasksPanel onInvoke={onTaskInvoke} />
        </div>
        {media && (
          <div class="mm-shell-left-body" role="tabpanel" hidden={tab !== "media"}>
            <MediaPanel api={media} importRequest={importRequest} />
          </div>
        )}
      </aside>
      <main class="mm-shell-center" />
      <aside class="mm-shell-right">
        <InspectorPanel layer={selection.layer} assets={selection.assets} onOp={sendOp} />
      </aside>
    </>
  );
};
