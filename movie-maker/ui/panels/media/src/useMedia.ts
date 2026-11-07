import { useCallback, useEffect, useState } from "preact/hooks";
import type { MediaApi, MediaItem } from "./api.ts";
import { IDLE_PROGRESS, reduceProgress, type ImportProgress } from "./progress.ts";

/** 購読を始める。解除は非同期で返るので、先に外れたら返ってきた時点で解除する。 */
const useSubscription = <T>(start: () => Promise<() => void>, deps: T[]): void => {
  useEffect(() => {
    let off: (() => void) | null = null;
    let alive = true;
    start().then((f) => {
      if (alive) off = f;
      else f();
    });
    return () => {
      alive = false;
      off?.();
    };
  }, deps);
};

export interface MediaState {
  items: MediaItem[];
  progress: ImportProgress;
  dragging: boolean;
  error: string | null;
  /** 値が変わるたびに `cache/` の大きさを読み直す。 */
  cacheKey: number;
  refresh: () => void;
  pickAndImport: () => void;
  dismissFailures: () => void;
}

/** 素材の一覧、取り込みの進み具合、ドロップの状態を持ち、アプリのイベントを購読する。 */
export const useMedia = (api: MediaApi, importRequest: number, refreshKey = 0): MediaState => {
  const [items, setItems] = useState<MediaItem[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [cacheKey, setCacheKey] = useState(0);
  const fail = (e: unknown) => setError(String(e));

  const refresh = useCallback(() => {
    api.list().then(setItems).catch(fail);
    setCacheKey((k) => k + 1);
  }, [api]);

  const startImport = useCallback(
    (paths: string[]) => {
      if (paths.length === 0) return;
      setError(null);
      api.importFiles(paths).catch(fail);
    },
    [api],
  );

  const pickAndImport = useCallback(() => {
    api.pickFiles().then(startImport).catch(fail);
  }, [api, startImport]);

  useEffect(refresh, [refresh, refreshKey]);
  useEffect(() => {
    if (importRequest > 0) pickAndImport();
  }, [importRequest]);

  const events = useAppEvents(api, refresh, startImport);

  return { items, error, cacheKey, refresh, pickAndImport, ...events };
};

/** 取り込みの進み具合とドロップのイベントを受ける。 */
const useAppEvents = (api: MediaApi, refresh: () => void, startImport: (paths: string[]) => void) => {
  const [progress, setProgress] = useState<ImportProgress>(IDLE_PROGRESS);
  const [dragging, setDragging] = useState(false);
  useSubscription(
    () =>
      api.onProgress((event) => {
        setProgress((p) => reduceProgress(p, event));
        if (event.type === "done" || event.type === "finished") refresh();
      }),
    [api, refresh],
  );
  useSubscription(
    () =>
      api.onDrop((event) => {
        setDragging(event.type === "enter");
        if (event.type === "drop") startImport(event.paths);
      }),
    [api, startImport],
  );
  return { progress, dragging, dismissFailures: () => setProgress(IDLE_PROGRESS) };
};
