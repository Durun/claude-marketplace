import { useCallback, useEffect, useState } from "preact/hooks";
import type { DeliverApi, DeliverOverview, ExportStatus, Op, RenderTarget } from "@mm/sdk";

const IDLE: ExportStatus = { running: false, items: [] };

/** 購読を始める。解除は非同期で返るので、先に外れたら返ってきた時点で解除する。 */
const useSubscription = (start: () => Promise<() => void>, deps: unknown[]): void => {
  useEffect(() => {
    let off: (() => void) | null = null;
    let alive = true;
    start().then(
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
  }, deps);
};

const message = (e: unknown): string => (e instanceof Error ? e.message : String(e));

/** 外して足す 1 つのトランザクションの操作。`id` が変わる編集も、取り消し 1 回で戻る。 */
export const replaceTargetOps = (before: string, after: RenderTarget): Op[] => [
  { op: "removeRenderTarget", target: before },
  { op: "addRenderTarget", target: after },
];

export interface DeliverState {
  overview: DeliverOverview | null;
  status: ExportStatus;
  /** 列に積んだレンダーターゲットの ID（積んだ順）。 */
  queue: string[];
  sequence: string;
  verify: boolean;
  error: string | null;
  toggleQueue: (id: string) => void;
  setSequence: (id: string) => void;
  setVerify: (on: boolean) => void;
  /** 操作を送る。断られたら理由を `error` に出して偽を返す。 */
  applyOps: (ops: Op[]) => Promise<boolean>;
  start: () => void;
  cancel: () => void;
  reveal: (path: string) => void;
  setError: (message: string | null) => void;
}

/** 列に積んだ順の選択、sequence、検証の有無。消えた対象は選択から外す。 */
const useSelection = (overview: DeliverOverview | null) => {
  const [queue, setQueue] = useState<string[]>([]);
  const [sequence, setSequence] = useState("");
  const [verify, setVerify] = useState(true);
  useEffect(() => {
    if (!overview) return;
    const ids = new Set(overview.targets.map((t) => t.id));
    setQueue((q) => (q.every((id) => ids.has(id)) ? q : q.filter((id) => ids.has(id))));
    setSequence((s) => (s === "" || overview.sequences.some((x) => x.id === s) ? s : ""));
    if (!overview.can_verify) setVerify(false);
  }, [overview]);
  const toggleQueue = (id: string) =>
    setQueue((q) => (q.includes(id) ? q.filter((x) => x !== id) : [...q, id]));
  return { queue, sequence, verify, toggleQueue, setSequence, setVerify };
};

/** 一覧、列の状態、積んだ順の選択を持ち、アプリの進み具合のイベントを購読する。 */
export const useDeliver = (api: DeliverApi, refreshKey: number): DeliverState => {
  const [overview, setOverview] = useState<DeliverOverview | null>(null);
  const [status, setStatus] = useState<ExportStatus>(IDLE);
  const [error, setError] = useState<string | null>(null);
  const selection = useSelection(overview);

  const refresh = useCallback(() => {
    api.overview().then(setOverview, (e) => setError(message(e)));
  }, [api]);
  useEffect(refresh, [refresh, refreshKey]);
  useEffect(() => {
    api.status().then(setStatus, () => undefined);
  }, [api]);
  useSubscription(() => api.onProgress(setStatus), [api]);

  const fail = (e: unknown) => setError(message(e));
  const applyOps = async (ops: Op[]): Promise<boolean> => {
    try {
      await api.applyOps(ops);
      setError(null);
      refresh();
      return true;
    } catch (e) {
      fail(e);
      return false;
    }
  };
  const start = () => {
    setError(null);
    const { queue, sequence, verify } = selection;
    api.start({ targets: queue, sequence: sequence === "" ? undefined : sequence, verify }).catch(fail);
  };
  return {
    ...selection,
    overview,
    status,
    error,
    applyOps,
    start,
    cancel: () => void api.cancel().catch(fail),
    reveal: (path) => void api.reveal(path).catch(fail),
    setError,
  };
};
