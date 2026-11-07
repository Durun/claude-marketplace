import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import type { MediaInvoke, MediaListen } from "@mm/sdk";

/** 素材の取り込みの状態。`ready` は作るべき出力が全部ある、`partial` は足りない、`missing` は元のファイルが無い。 */
export type MediaStatus = "ready" | "partial" | "missing";

/** 素材の種類。`unknown` は ffprobe が使えず調べられなかった。 */
export type MediaKind = "video" | "audio" | "unknown";

/** 素材の一覧の 1 行（アプリの `media_list`）。時間は秒。 */
export interface MediaItem {
  id: string;
  name: string;
  path: string;
  content_hash: string;
  kind: MediaKind;
  duration: number;
  fps: number | null;
  width: number | null;
  height: number | null;
  has_thumbnail: boolean;
  has_waveform: boolean;
  status: MediaStatus;
}

/** 間引いた波形（アプリの `media_waveform`）。ピークは 0 から 1。 */
export interface WaveformView {
  duration: number;
  peaks: number[];
}

/** `cache/` の下のフォルダー 1 つ。`clearable` が偽のものは掃除できない。 */
export interface CacheFolder {
  name: string;
  bytes: number;
  files: number;
  clearable: boolean;
}

/** `cache/` の大きさ（アプリの `media_cache_info`）。 */
export interface CacheInfo {
  total_bytes: number;
  folders: CacheFolder[];
}

/** 取り込みの進み具合（Tauri のイベント `mm:ingest-progress`）。 */
export type ImportEvent =
  | { type: "started"; index: number; total: number; path: string }
  | { type: "done"; index: number; path: string; asset_id: string; added: boolean }
  | { type: "failed"; index: number; path: string; error: string }
  | { type: "finished"; added: number; skipped: number; failed: number };

/** ウィンドウへのファイルのドラッグ・アンド・ドロップ。 */
export type DropEvent = { type: "enter" } | { type: "leave" } | { type: "drop"; paths: string[] };

/** 取り込みの進み具合を送る Tauri のイベント名。アプリの `media_ipc.rs` と同じ。 */
export const INGEST_PROGRESS_EVENT = "mm:ingest-progress";

/** Tauri がファイルのドロップを知らせるイベント名。 */
export const DRAG_ENTER_EVENT = "tauri://drag-enter";
export const DRAG_LEAVE_EVENT = "tauri://drag-leave";
export const DRAG_DROP_EVENT = "tauri://drag-drop";

/** Media パネルがアプリに頼むこと。テストでは差し替える。 */
export interface MediaApi {
  list: () => Promise<MediaItem[]>;
  /** サムネイルの JPEG のバイト列。 */
  thumbnail: (id: string) => Promise<ArrayBuffer>;
  waveform: (id: string, buckets: number) => Promise<WaveformView>;
  /** ファイルの選択ダイアログを開く。キャンセルは空の配列。 */
  pickFiles: () => Promise<string[]>;
  /** 取り込みが終わるまで待つ。進み具合は `onProgress` に届く。 */
  importFiles: (paths: string[]) => Promise<void>;
  cacheInfo: () => Promise<CacheInfo>;
  /** 掃除して空けたバイト数を返す。 */
  clearCache: (kinds: string[]) => Promise<number>;
  onProgress: (handler: (event: ImportEvent) => void) => Promise<() => void>;
  onDrop: (handler: (event: DropEvent) => void) => Promise<() => void>;
}

interface DropPayload {
  paths?: string[];
}

/** Tauri の IPC で Media のコマンドを呼ぶ実装。コマンドの名前と引数は ui/sdk/README.md。 */
export const createIpcMediaApi = (
  call: MediaInvoke = tauriInvoke,
  listen: MediaListen = tauriListen,
): MediaApi => ({
  list: async () => (await call("media_list")) as MediaItem[],
  thumbnail: async (id) => (await call("media_thumbnail", { id })) as ArrayBuffer,
  waveform: async (id, buckets) => (await call("media_waveform", { id, buckets })) as WaveformView,
  pickFiles: async () => (await call("media_pick_files")) as string[],
  importFiles: async (paths) => {
    await call("media_import", { paths });
  },
  cacheInfo: async () => (await call("media_cache_info")) as CacheInfo,
  clearCache: async (kinds) => (await call("media_cache_clear", { kinds })) as number,
  onProgress: (handler) => listen(INGEST_PROGRESS_EVENT, (e) => handler(e.payload as ImportEvent)),
  onDrop: async (handler) => {
    const off = await Promise.all([
      listen(DRAG_ENTER_EVENT, () => handler({ type: "enter" })),
      listen(DRAG_LEAVE_EVENT, () => handler({ type: "leave" })),
      listen(DRAG_DROP_EVENT, (e) =>
        handler({ type: "drop", paths: (e.payload as DropPayload).paths ?? [] }),
      ),
    ]);
    return () => off.forEach((f) => f());
  },
});
