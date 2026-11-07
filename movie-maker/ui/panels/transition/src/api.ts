import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { createIpcTransport } from "@mm/sdk";

/** カットの境に置いてあるトランジション。 */
export interface CutTransition {
  /** パックのトランジションの名前。パックのコードと合わなければ `null`。 */
  kind: string | null;
  /** 長さ（秒）。 */
  duration: number;
}

/** sequence のカット 1 つ（アプリの `transition_cuts`）。時間は秒。 */
export interface TransitionCut {
  id: string;
  start: number;
  duration: number;
  /** 前のカットとの境のトランジション。 */
  transition: CutTransition | null;
}

/** 開いているコンポジションの sequence とそのカット。sequence が無ければ `sequence` は `null`。 */
export interface TransitionState {
  sequence: string | null;
  cuts: TransitionCut[];
}

/** Transition パネルがアプリに頼むこと。テストでは差し替える。失敗は reject（理由の文字を持つ Error）。 */
export interface TransitionApi {
  cuts: () => Promise<TransitionState>;
  /** パックのトランジションの名前。 */
  kinds: () => Promise<string[]>;
  /** カットの頭に `kind` を `duration` 秒で置く。置いてあれば差し替える（`setTransition` 1 回）。 */
  set: (cut: string, kind: string, duration: number) => Promise<void>;
  /** カットの頭のトランジションを外す（`setTransition` 1 回）。 */
  remove: (cut: string) => Promise<void>;
}

/** パックのフォルダー。アプリの作業フォルダー（movie-maker/）からの相対。 */
export const DEFAULT_PACK_DIR = "packs/vlog";

/** マクロ `transition`（`packs/vlog/macros/transition.json`）の名前。 */
const TRANSITION_MACRO = "transition";

/** Tauri の invoke と同じ形の関数。テストで差し替える。 */
export type TransitionInvoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

/** Tauri の失敗（文字列）を、理由を持つ Error にする。 */
const asError = (e: unknown): Error => (e instanceof Error ? e : new Error(String(e)));

/** Tauri の IPC で Transition のコマンドを呼ぶ実装。コマンドの名前と引数は ui/sdk/README.md。 */
export const createIpcTransitionApi = (
  call: TransitionInvoke = tauriInvoke,
  dir: string = DEFAULT_PACK_DIR,
): TransitionApi => {
  const run = async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
    try {
      return (await call(command, args)) as T;
    } catch (e) {
      throw asError(e);
    }
  };
  const transport = createIpcTransport((command, args) => call(command, args));
  return {
    cuts: () => run("transition_cuts", { dir }),
    kinds: () => run("pack_transitions", { dir }),
    set: async (cut, kind, duration) => {
      await run("pack_apply", { dir, name: TRANSITION_MACRO, args: { cut, kind, dur: String(duration) } });
    },
    remove: async (cut) => {
      const result = await transport.send({ op: "setTransition", cut, transition: null });
      if (!result.ok) throw new Error(result.error);
    },
  };
};
