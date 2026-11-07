import { listen as tauriListen } from "@tauri-apps/api/event";
import type { MediaListen } from "./ipc.ts";

/** プロジェクトのデータが新しくなったことを知らせる Tauri のイベント名。アプリの `app.rs` と同じ。 */
export const DOC_CHANGED_EVENT = "mm:doc-changed";

/** `mm:doc-changed` の中身。`actor` は編集した側（人、CLI など）。 */
export interface DocChanged {
  version: number;
  txn_id: string;
  kind: string;
  name: string;
  actor: string;
  touched: string[];
}

/** プロジェクトのデータが変わるたびに `handler` を呼ぶ。返り値で購読を止める。 */
export const onDocChanged = (
  handler: (change: DocChanged) => void,
  listen: MediaListen = tauriListen,
): Promise<() => void> => listen(DOC_CHANGED_EVENT, (e) => handler(e.payload as DocChanged));
