import { useCallback, useEffect, useState } from "preact/hooks";
import type { Op, RenderTarget } from "@mm/sdk";
import type { DeliverApi, DeliverOverview, ExportStatus } from "./api.ts";

const IDLE: ExportStatus = { running: false, items: [] };

/**
 * キューの状態を、購読を先に始めてから読む。読んだ応答がイベントより後に届いても、
 * 先に受けたイベントの新しい状態を古い状態で上書きしない。解除は非同期で返るので、先に外れたら返ってきた時点で解除する。
 */
const useExportStatus = (api: DeliverApi, setStatus: (s: ExportStatus) => void): void => {
  useEffect(() => {
    let off: (() => void) | null = null;
    let alive = true;
    let sawEvent = false;
    api
      .onProgress((s) => {
        sawEvent = true;
        setStatus(s);
      })
      .then(
        (f) => {
          if (!alive) return f();
          off = f;
          api.status().then(
            (s) => {
              if (alive && !sawEvent) setStatus(s);
            },
            () => undefined,
          );
        },
        () => undefined,
      );
    return () => {
      alive = false;
      off?.();
    };
  }, [api]);
};

const message = (e: unknown): string => (e instanceof Error ? e.message : String(e));

/** 外して同じ位置に足す 1 つのトランザクションの操作。`id` が変わる編集も、取り消し 1 回で戻る。 */
export const replaceTargetOps = (before: string, after: RenderTarget, index: number): Op[] => [
  { op: "removeRenderTarget", target: before },
  { op: "addRenderTarget", target: after, index },
];

export interface DeliverState {
  overview: DeliverOverview | null;
  status: ExportStatus;
  /** 書き出しの依頼を送って、アプリが受けるまで。この間はもう一度押せない。 */
  starting: boolean;
  /** キューに積んだレンダーターゲットの ID（積んだ順）。 */
  queue: string[];
  sequence: string;
  verify: boolean;
  error: string | null;
  toggleQueue: (id: string) => void;
  setSequence: (id: string) => void;
  setVerify: (on: boolean) => void;
  /** 操作を送る。拒否されたら理由を `error` に出して偽を返す。 */
  applyOps: (ops: Op[]) => Promise<boolean>;
  start: () => void;
  cancel: () => void;
  reveal: (path: string) => void;
  setError: (message: string | null) => void;
}

/** キューに積んだ順の選択、sequence、検証の有無。消えた対象は選択から外す。 */
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

/** 一覧、キューの状態、積んだ順の選択を持ち、アプリの進み具合のイベントを購読する。 */
export const useDeliver = (api: DeliverApi, refreshKey: number): DeliverState => {
  const [overview, setOverview] = useState<DeliverOverview | null>(null);
  const [status, setStatus] = useState<ExportStatus>(IDLE);
  const [error, setError] = useState<string | null>(null);
  const selection = useSelection(overview);

  const refresh = useCallback(() => {
    api.overview().then(setOverview, (e) => setError(message(e)));
  }, [api]);
  useEffect(refresh, [refresh, refreshKey]);
  const [starting, setStarting] = useState(false);
  useExportStatus(api, setStatus);

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
    if (starting) return;
    setError(null);
    setStarting(true);
    const { queue, sequence, verify } = selection;
    api
      .start({ targets: queue, sequence: sequence === "" ? undefined : sequence, verify })
      .catch(fail)
      .finally(() => setStarting(false));
  };
  return {
    ...selection,
    overview,
    status,
    starting,
    error,
    applyOps,
    start,
    cancel: () => void api.cancel().catch(fail),
    reveal: (path) => void api.reveal(path).catch(fail),
    setError,
  };
};
