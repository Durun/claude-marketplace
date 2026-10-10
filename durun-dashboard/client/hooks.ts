import { createContext } from 'preact';
import { useEffect, useRef, useState } from 'preact/hooks';
import type { Snapshot } from '../shared/types.ts';
import { getJson } from './api.ts';

const NOW_TICK_MS = 30_000;

/** スナップショットが来ない間も、相対時刻の表示を進めるための現在時刻。 */
export const NowContext = createContext(Date.now());

export const useNow = (): number => {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), NOW_TICK_MS);
    return () => clearInterval(timer);
  }, []);
  return now;
};

const CACHE_KEY = 'snapshot';

const cachedSnapshot = (): Snapshot | null => {
  try {
    return JSON.parse(localStorage.getItem(CACHE_KEY) ?? 'null') as Snapshot | null;
  } catch {
    return null;
  }
};

/**
 * snapshot は画面に出すデータで、サーバーから届くまでは、前に開いていたときのデータを出す。
 * fresh はサーバーから届いたものだけで、通知や読み込み直しのように、古いデータで動くと困る処理に使う。
 * 軽量モードでは、サーバーに間隔を空けて配らせ、タブが裏にある間は受け取らない。そのため、裏にある間は通知も出ない。
 */
export const useSnapshot = (light: boolean): { snapshot: Snapshot | null; fresh: Snapshot | null; connected: boolean } => {
  const [fresh, setFresh] = useState<Snapshot | null>(null);
  const [cached] = useState(cachedSnapshot);
  const [connected, setConnected] = useState(true);
  const latest = useRef(fresh);
  latest.current = fresh;
  useEffect(() => {
    // 取得の応答が、後から届いた SSE より古いことがあるため、新しい方を残す。
    const accept = (next: Snapshot) => setFresh((current) => (current && current.now > next.now ? current : next));
    // SSE は変化があるまで何も送らないことがある。最初と再接続のたびに取得して、古い表示を残さない。
    const load = () => getJson<Snapshot>('/api/snapshot').then(accept).catch(() => setConnected(false));
    load();
    let source: EventSource | undefined;
    const open = () => {
      source = new EventSource(light ? '/api/events?light' : '/api/events');
      source.onopen = () => {
        setConnected(true);
        load();
      };
      source.onmessage = (e) => accept(JSON.parse(e.data) as Snapshot);
      source.onerror = () => setConnected(false);
    };
    const close = () => {
      source?.close();
      source = undefined;
    };
    // 表に戻ったら、つなぎ直して最新のものを受け取る。
    const onVisibility = () => {
      if (document.hidden) close();
      else if (!source) open();
    };
    if (light) document.addEventListener('visibilitychange', onVisibility);
    if (!light || !document.hidden) open();
    // 閉じるときと読み込み直すときに残し、次に開いたときにすぐ出す。
    const save = () => {
      try {
        if (latest.current) localStorage.setItem(CACHE_KEY, JSON.stringify(latest.current));
      } catch {
        // 保存できる大きさを超えたら、次に開いたときはサーバーから届くまで待つ。
      }
    };
    addEventListener('pagehide', save);
    return () => {
      close();
      document.removeEventListener('visibilitychange', onVisibility);
      removeEventListener('pagehide', save);
    };
  }, [light]);
  return { snapshot: fresh ?? cached, fresh, connected };
};

type FetchState<T> = { data?: T; error?: string; loading: boolean; reload: () => void };

/** 取得中も直前の結果を残す。画面が一瞬空にならないようにするため。 */
export const useFetch = <T,>(load: () => Promise<T>, deps: unknown[]): FetchState<T> => {
  const [state, setState] = useState<{ data?: T; error?: string; loading: boolean }>({ loading: true });
  const [tick, setTick] = useState(0);
  useEffect(() => {
    let alive = true;
    setState((s) => ({ ...s, loading: true, error: undefined }));
    load().then(
      (data) => alive && setState({ data, loading: false }),
      (e: unknown) => alive && setState((s) => ({ ...s, loading: false, error: e instanceof Error ? e.message : String(e) })),
    );
    return () => {
      alive = false;
    };
  }, [...deps, tick]);
  return { ...state, reload: () => setTick((t) => t + 1) };
};
