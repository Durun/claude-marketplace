import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type { Op, OpArgs, OpName, OpOf } from "./generated/ops.ts";

/** 操作を送った結果。成功なら value（アプリが返す値）、失敗なら error を持つ。 */
export type OpResult = { ok: true; value: unknown } | { ok: false; error: string };

/** 操作をアプリに届ける裏。IPC と HTTP のどちらも同じ形で差し替える。 */
export interface Transport {
  send: (op: Op) => Promise<OpResult>;
}

/** Tauri の命令の名前。アプリ側の受け口はこの名前で `Op` をそのまま受ける（ui/sdk/README.md）。 */
export const IPC_APPLY_OP = "apply_op";

/** Tauri の invoke と同じ形の関数。テストで差し替える。 */
export type TauriInvoke = (command: string, args: { op: Op }) => Promise<unknown>;

const errorText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

/** Tauri の IPC で操作を送る裏。命令が拒否（Err）したら、その文字列を error にする。 */
export const createIpcTransport = (call: TauriInvoke = tauriInvoke): Transport => ({
  send: async (op) => {
    try {
      return { ok: true, value: await call(IPC_APPLY_OP, { op }) };
    } catch (e) {
      return { ok: false, error: errorText(e) };
    }
  },
});

/** 操作の名前と引数から `Op` を作って送る窓口。 */
export interface OpClient {
  /** 操作の名前と引数を型つきで送る。 */
  send: <N extends OpName>(name: N, args: OpArgs<N>) => Promise<OpResult>;
  /** 組み立て済みの `Op` を送る。 */
  sendOp: (op: Op) => Promise<OpResult>;
}

/** 裏を 1 つ受け取り、型つきの送る関数を返す。 */
export const createClient = (transport: Transport): OpClient => ({
  send: (name, args) => transport.send({ op: name, ...args } as OpOf<typeof name>),
  sendOp: (op) => transport.send(op),
});
