import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import type { MediaInvoke, MediaListen, Op } from "@mm/sdk";

/** 提案が追加・決定されたことを知らせる Tauri のイベント名。アプリの `review_ipc.rs` と同じ。 */
export const PROPOSALS_CHANGED_EVENT = "mm:proposals-changed";

export type ProposalState = "pending" | "approved" | "rejected";

/** 提案の操作 1 つ。`index` は提案の中の位置で、一部だけ承認するときの指定に使う。 */
export interface ProposalOp {
  index: number;
  op: Op;
  /** この操作が触る ID。 */
  touches: string[];
}

/** 提案が触る範囲。`start` と `end` はシーケンスの時間軸の秒。 */
export interface ProposalScope {
  sequences: { id: string; cuts: string[]; start: number; end: number }[];
  layers: { comp: string; id: string }[];
  other: string[];
}

/** Claude が出した編集の提案（アプリの `review_list`）。保留中はプロジェクトのデータに入っていない。 */
export interface Proposal {
  id: number;
  name: string;
  /** 操作経路の種類（`mcp`、`cli`）。 */
  actor: string;
  /** 操作した主体の名前（`claude`）。 */
  actor_name: string;
  at_ms: number;
  /** 対応しているコメントの ID。 */
  responds_to: string | null;
  base_version: number;
  state: ProposalState;
  decided_by: string | null;
  decided_at_ms: number | null;
  txn_id: number | null;
  approved_ops: number[] | null;
  /** 承認で適用したトランザクションが、今も取り消されていない。 */
  applied: boolean;
  /** そのトランザクションが取り消しのスタックの一番上にある。 */
  undoable: boolean;
  /** 保留中で、提案の後の編集と触る所が重なる ID。あれば承認できない。 */
  conflict: string[] | null;
  ops: ProposalOp[];
  scope: ProposalScope;
}

/** `review_approve` の結果。`change` は適用したトランザクションの変更（`mm:doc-changed` と同じ形）。 */
export interface ApproveResult {
  proposal: Proposal;
  change: unknown;
}

/** `mm:proposals-changed` の中身。 */
export interface ProposalsChanged {
  id: number;
  state: string;
}

/** Review パネルがアプリに頼むこと。テストでは差し替える。 */
export interface ReviewApi {
  /** 提案の一覧（新しい順）。 */
  list: () => Promise<Proposal[]>;
  /** 承認する。`ops` は承認する操作の index で、`null` は全部。 */
  approve: (id: number, ops: number[] | null) => Promise<ApproveResult>;
  reject: (id: number) => Promise<Proposal>;
  /** 却下した提案を保留に戻す。 */
  reopen: (id: number) => Promise<Proposal>;
  /** 承認で適用したトランザクションが取り消しのスタックの一番上のときだけ取り消す。 */
  undo: (id: number) => Promise<Proposal>;
  /** Program に提案を適用したフレームを出す。`id` が `null` で元のフレームに戻す。 */
  preview: (id: number | null, ops: number[] | null) => Promise<void>;
  onChanged: (handler: (change: ProposalsChanged) => void) => Promise<() => void>;
}

/** Tauri の失敗（文字列）を、理由を持つ Error にする。 */
const asError = (e: unknown): Error => (e instanceof Error ? e : new Error(String(e)));

/** Tauri の IPC で Review のコマンドを呼ぶ実装。コマンドの名前と引数は ui/sdk/README.md。 */
export const createIpcReviewApi = (
  call: MediaInvoke = tauriInvoke,
  listen: MediaListen = tauriListen,
): ReviewApi => {
  const run = async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
    try {
      return (await call(command, args)) as T;
    } catch (e) {
      throw asError(e);
    }
  };
  return {
    list: () => run("review_list"),
    approve: (id, ops) => run("review_approve", { id, ops }),
    reject: (id) => run("review_reject", { id }),
    reopen: (id) => run("review_reopen", { id }),
    undo: (id) => run("review_undo", { id }),
    preview: async (id, ops) => {
      await run("review_preview", { id, ops });
    },
    onChanged: (handler) => listen(PROPOSALS_CHANGED_EVENT, (e) => handler(e.payload as ProposalsChanged)),
  };
};
