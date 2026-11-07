import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import type { Proposal, ReviewApi } from "@mm/sdk";

const message = (e: unknown): string => (e instanceof Error ? e.message : String(e));

/** 提案ごとの、承認する操作の index。載っていない提案は全部オン。 */
export type Selections = Record<number, number[] | undefined>;

/** 選んだ操作の index。全部のときは `null`（アプリへの指定も `null` が全部）。 */
export const chosenOps = (p: Proposal, selections: Selections): number[] | null => {
  const picked = selections[p.id];
  return picked && picked.length < p.ops.length ? picked : null;
};

/** チェックの入っている操作の index を、提案の操作の並びで返す。 */
export const checkedIndexes = (p: Proposal, selections: Selections): number[] =>
  selections[p.id] ?? p.ops.map((o) => o.index);

/** 一覧を読み、`refreshKey` の変化、提案のイベント、`mm:doc-changed` での読み直しを受ける。 */
const useProposals = (api: ReviewApi, refreshKey: number, onError: (m: string | null) => void) => {
  const [proposals, setProposals] = useState<Proposal[] | null>(null);
  const reload = useCallback(async () => {
    try {
      setProposals(await api.list());
    } catch (e) {
      onError(message(e));
    }
  }, [api, onError]);
  useEffect(() => {
    void reload();
  }, [reload, refreshKey]);
  useEffect(() => {
    let off: (() => void) | null = null;
    let alive = true;
    api.onChanged(() => void reload()).then(
      (f) => {
        if (alive) off = f;
        else f();
      },
      () => undefined,
    );
    return () => {
      alive = false;
      off?.();
    };
  }, [api, reload]);
  return { proposals, reload };
};

/** 1 つの提案だけが Proposed になれる。いまの選択を Program に当てた絵に送る。 */
const usePreview = (api: ReviewApi, onError: (m: string | null) => void) => {
  const [previewId, setPreviewId] = useState<number | null>(null);
  const active = useRef<number | null>(null);
  const show = useCallback(
    async (id: number | null, ops: number[] | null) => {
      active.current = id;
      setPreviewId(id);
      try {
        await api.preview(id, ops);
      } catch (e) {
        onError(message(e));
      }
    },
    [api, onError],
  );
  // 別のパネルへ移ったあとも Program に提案の絵を残さない。
  useEffect(
    () => () => {
      if (active.current !== null) void api.preview(null, null).catch(() => undefined);
    },
    [api],
  );
  return { previewId, show };
};

export interface ReviewState {
  proposals: Proposal[] | null;
  error: string | null;
  selections: Selections;
  previewId: number | null;
  toggleOp: (p: Proposal, index: number) => void;
  setPreviewing: (p: Proposal, on: boolean) => void;
  approve: (p: Proposal) => void;
  reject: (p: Proposal) => void;
  reopen: (p: Proposal) => void;
  undo: (p: Proposal) => void;
}

/** Review パネルの状態と、アプリへの頼みごと。失敗の理由は `error` に残す。 */
export const useReview = (api: ReviewApi, refreshKey = 0): ReviewState => {
  const [error, setError] = useState<string | null>(null);
  const [selections, setSelections] = useState<Selections>({});
  const { proposals, reload } = useProposals(api, refreshKey, setError);
  const { previewId, show } = usePreview(api, setError);

  // 決まった・消えた提案の絵は残さない。
  useEffect(() => {
    if (previewId === null || !proposals) return;
    if (!proposals.some((p) => p.id === previewId && p.state === "pending")) void show(null, null);
  }, [proposals, previewId, show]);

  const decide = (action: () => Promise<unknown>) => {
    setError(null);
    action().then(reload, (e) => {
      setError(message(e));
      void reload();
    });
  };

  const toggleOp = (p: Proposal, index: number) => {
    const now = checkedIndexes(p, selections);
    const next = now.includes(index) ? now.filter((i) => i !== index) : [...now, index].sort((a, b) => a - b);
    setSelections({ ...selections, [p.id]: next });
    if (previewId === p.id) void show(p.id, chosenOps(p, { ...selections, [p.id]: next }));
  };

  return {
    proposals,
    error,
    selections,
    previewId,
    toggleOp,
    setPreviewing: (p, on) => void (on ? show(p.id, chosenOps(p, selections)) : show(null, null)),
    approve: (p) => decide(() => api.approve(p.id, chosenOps(p, selections))),
    reject: (p) => decide(() => api.reject(p.id)),
    reopen: (p) => decide(() => api.reopen(p.id)),
    undo: (p) => decide(() => api.undo(p.id)),
  };
};
