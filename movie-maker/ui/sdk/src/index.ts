/** 操作の呼び出しの結果。成功なら value、失敗なら error を持つ。 */
export type InvokeResult<T = unknown> = { ok: true; value: T } | { ok: false; error: string };

/** UI のコマンドの呼び出し。command は Tasks パネルなどの項目の名前で、mm-ops の操作ではない。 */
export type Invoke = (command: string, args?: Record<string, unknown>) => Promise<InvokeResult>;

/**
 * UI のコマンドを呼ぶ関数の形。
 * コマンドのハンドラーがアプリに出来るまでの仮の中身で、呼ばれた名前を記録して失敗を返す。
 */
export const invoke: Invoke = async (command, args) => {
  console.debug("[mm-sdk] invoke", command, args ?? {});
  return { ok: false, error: `sdk is not connected to mm-ops yet: ${command}` };
};

export * from "./generated/ops.ts";
export {
  createClient,
  createIpcTransport,
  IPC_APPLY_OP,
  type OpClient,
  type OpResult,
  type TauriInvoke,
  type Transport,
} from "./client.ts";
export type { JsonSchema, SchemaObject } from "./codegen.ts";
export * from "./keyframes.ts";
export type { MediaInvoke, MediaListen } from "./ipc.ts";
export { definePanel, type AnyPanel, type PanelColumn, type PanelContext, type PanelDescriptor, type LayerSelection } from "./panel.ts";
export { DOC_CHANGED_EVENT, onDocChanged, type DocChanged } from "./docChanged.ts";
export { ASSET_SELECTED_EVENT, announceAsset } from "./events.ts";
