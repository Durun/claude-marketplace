import type { UnlistenFn } from "@tauri-apps/api/event";

/** Tauri の invoke と同じ形の関数。パネルの API の IPC 実装とテストが差し替える。 */
export type MediaInvoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

/** Tauri の listen と同じ形の関数。パネルの API の IPC 実装とテストが差し替える。 */
export type MediaListen = (
  event: string,
  handler: (e: { payload: unknown }) => void,
) => Promise<UnlistenFn>;
