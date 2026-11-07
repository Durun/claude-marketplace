import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import type { MediaInvoke, MediaListen, Op, RenderTarget } from "@mm/sdk";

/** コンポジション 1 つ。レンダーターゲットの `composition` の選択肢と、範囲の表示に使う。時間は秒。 */
export interface DeliverComposition {
  id: string;
  duration: number;
  /** 画素。縦長のレンダーターゲット（Shorts）の既定のコンポジションを選ぶのに使う。 */
  width: number;
  height: number;
}

/** sequence 1 つ。選ぶとレンダーターゲットのコンポジションの代わりに、全長を書き出す。時間は秒。 */
export interface DeliverSequence {
  id: string;
  duration: number;
}

/** Deliver を開いたときの一覧（アプリの `export_overview`）。 */
export interface DeliverOverview {
  targets: RenderTarget[];
  compositions: DeliverComposition[];
  sequences: DeliverSequence[];
  /** いま開いているコンポジション。レンダーターゲットを足すときの既定の `composition`。 */
  open_composition: string | null;
  /** 書き出したあとの ffprobe の検証が使えるか。 */
  can_verify: boolean;
}

/** キューの 1 項目の状態。 */
export type ExportItemState = "queued" | "running" | "done" | "failed" | "cancelled";

/** ffprobe が読んだ出力の中身。 */
export interface ExportVerification {
  video_codec: string;
  width: number;
  height: number;
  video_frames: number;
  video_seconds: number;
  audio_seconds: number | null;
}

/** キューの 1 項目。時間は秒。 */
export interface ExportItem {
  target: string;
  state: ExportItemState;
  frames_done: number;
  frames_total: number;
  fps: number;
  elapsed: number;
  /** 残りの時間の見込み。まだ測れないときは `null`。 */
  eta: number | null;
  output_path: string;
  bytes_written: number;
  verification: ExportVerification | null;
  error: string | null;
}

/** 書き出しのキュー全体の状態（アプリの `export_status` と、イベント `mm:export-progress` の中身）。 */
export interface ExportStatus {
  running: boolean;
  items: ExportItem[];
}

/** 書き出しの依頼（アプリの `export_start`）。 */
export interface ExportRequest {
  /** 書き出すレンダーターゲットの ID。この順に書き出す。 */
  targets: string[];
  /** 指すとレンダーターゲットのコンポジションの代わりに、この sequence を全長で書き出す。 */
  sequence?: string;
  /** 書き出したあとに ffprobe で検証する。 */
  verify: boolean;
}

/** 書き出しのキューの状態を送る Tauri のイベント名。アプリの `deliver_ipc.rs` と同じ。 */
export const EXPORT_PROGRESS_EVENT = "mm:export-progress";

/** Deliver パネルがアプリに頼むこと。テストでは差し替える。失敗は reject（理由の文字を持つ Error）。 */
export interface DeliverApi {
  overview: () => Promise<DeliverOverview>;
  status: () => Promise<ExportStatus>;
  /** 操作を 1 つのトランザクションで送る（`apply_ops`）。取り消し 1 回で戻る。 */
  applyOps: (ops: Op[]) => Promise<void>;
  /** キューを作って書き出しを始め、すぐ返る。進み具合は `onProgress`。 */
  start: (request: ExportRequest) => Promise<void>;
  cancel: () => Promise<void>;
  /** 書き出したファイルをエクスプローラーで選んで開く。 */
  reveal: (path: string) => Promise<void>;
  /** 出力先の保存ダイアログ。`codec` に合う拡張子で絞る。キャンセルは `null`。 */
  pickOutput: (current?: string, codec?: string) => Promise<string | null>;
  onProgress: (handler: (status: ExportStatus) => void) => Promise<() => void>;
}

/** Tauri の失敗（文字列）を、理由を持つ Error にする。 */
const asError = (e: unknown): Error => (e instanceof Error ? e : new Error(String(e)));

/** Tauri の IPC で Deliver のコマンドを呼ぶ実装。コマンドの名前と引数は ui/sdk/README.md。 */
export const createIpcDeliverApi = (
  call: MediaInvoke = tauriInvoke,
  listen: MediaListen = tauriListen,
): DeliverApi => {
  const run = async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
    try {
      return (await call(command, args)) as T;
    } catch (e) {
      throw asError(e);
    }
  };
  return {
    overview: () => run("export_overview"),
    status: () => run("export_status"),
    applyOps: async (ops) => {
      await run("apply_ops", { ops });
    },
    start: async (request) => {
      await run("export_start", { request });
    },
    cancel: async () => {
      await run("export_cancel");
    },
    reveal: async (path) => {
      await run("export_reveal", { path });
    },
    pickOutput: (current, codec) => run("export_pick_output", { current: current ?? null, codec: codec ?? null }),
    onProgress: (handler) => listen(EXPORT_PROGRESS_EVENT, (e) => handler(e.payload as ExportStatus)),
  };
};
