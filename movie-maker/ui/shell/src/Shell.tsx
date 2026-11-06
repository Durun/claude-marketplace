import { useEffect, useState } from "preact/hooks";
import { InspectorPanel } from "@mm/inspector";
import { invoke, type Asset, type Layer, type Op } from "@mm/sdk";
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
}

/** 左に Tasks、右に Inspector。 */
export const Shell = ({ sendOp }: ShellProps) => {
  const [selection, setSelection] = useState<LayerSelection>({ layer: null });
  useEffect(() => {
    const onSelected = (e: Event) => setSelection((e as CustomEvent<LayerSelection>).detail);
    window.addEventListener(LAYER_SELECTED_EVENT, onSelected);
    return () => window.removeEventListener(LAYER_SELECTED_EVENT, onSelected);
  }, []);
  return (
    <>
      <aside class="mm-shell-left">
        <TasksPanel onInvoke={(command) => void invoke(command)} />
      </aside>
      <main class="mm-shell-center" />
      <aside class="mm-shell-right">
        <InspectorPanel layer={selection.layer} assets={selection.assets} onOp={sendOp} />
      </aside>
    </>
  );
};
