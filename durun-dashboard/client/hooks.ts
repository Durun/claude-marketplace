import { createContext } from 'preact';
import { useContext, useEffect, useRef, useState } from 'preact/hooks';
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

/** SSE を張っているタブが、ほかのタブに配るもの。 */
type EventsMessage = { kind: 'open' } | { kind: 'error' } | { kind: 'data'; data: string };

// useSnapshot がサーバーから取り直す関数。useSnapshot を使っている間だけある。
let loadSnapshot: (() => void) | undefined;

/** SSE が変化を送るのを待たずに、サーバーにスナップショットを作らせて受け取る。 */
export const reloadSnapshot = (): void => loadSnapshot?.();

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
    loadSnapshot = load;
    load();
    const url = light ? '/api/events?light' : '/api/events';
    // ブラウザは同じホストへの接続を 6 本までしか張らないので、タブごとに SSE を握ると会話ログなどの取得が空きを待たされる。
    // SSE を張るのは 1 つのタブだけにし、受け取ったものは BroadcastChannel でほかのタブに配る。そのタブが閉じると、ほかのタブが引き継ぐ。
    const channel = new BroadcastChannel(url);
    const receive = (m: EventsMessage) => {
      if (m.kind === 'open') {
        setConnected(true);
        load();
      } else if (m.kind === 'error') setConnected(false);
      else accept(JSON.parse(m.data) as Snapshot);
    };
    let listening: AbortController | undefined;
    const open = () => {
      const abort = new AbortController();
      listening = abort;
      channel.onmessage = (e: MessageEvent<EventsMessage>) => receive(e.data);
      const lead = (share: boolean) =>
        new Promise<void>((release) => {
          const source = new EventSource(url);
          const relay = (m: EventsMessage) => {
            receive(m);
            if (share) channel.postMessage(m);
          };
          source.onopen = () => relay({ kind: 'open' });
          source.onmessage = (e: MessageEvent<string>) => relay({ kind: 'data', data: e.data });
          source.onerror = () => relay({ kind: 'error' });
          abort.signal.addEventListener('abort', () => {
            source.close();
            release();
          });
        });
      // localhost 以外を http で開くと、型では常にある Web Locks が無い。そのときはタブごとに SSE を張り、ほかのタブには配らない。
      if (!navigator.locks) {
        void lead(false);
        return;
      }
      // 順番待ちの間に止めると AbortError で終わる。止めたのはこのタブなので、知らせることはない。
      navigator.locks.request(url, { signal: abort.signal }, () => lead(true)).catch(() => {});
    };
    const close = () => {
      listening?.abort();
      listening = undefined;
      channel.onmessage = null;
    };
    // 表に戻ったら、つなぎ直して最新のものを受け取る。
    const onVisibility = () => {
      if (document.hidden) close();
      else if (!listening) open();
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
      loadSnapshot = undefined;
      close();
      channel.close();
      document.removeEventListener('visibilitychange', onVisibility);
      removeEventListener('pagehide', save);
    };
  }, [light]);
  return { snapshot: fresh ?? cached, fresh, connected };
};

type FetchState<T> = { data?: T; error?: string; loading: boolean; reload: () => void };

/**
 * 取得中も直前の結果を残す。画面が一瞬空にならないようにするため。
 * load に渡す fresh は、reload で読み直したときに true になる。サーバーのキャッシュを通す API には、覚えた値を使わずに読み直させる。
 * version だけが変わったときは、読み込み中にせずに取り直す。サーバーが裏で読み直した値に差し替えるだけだからである。
 */
export const useFetch = <T,>(load: (fresh: boolean) => Promise<T>, deps: unknown[], version?: number): FetchState<T> => {
  const [state, setState] = useState<{ data?: T; error?: string; loading: boolean }>({ loading: true });
  const [tick, setTick] = useState(0);
  const forced = useRef(false);
  const lastDeps = useRef<unknown[]>();
  useEffect(() => {
    let alive = true;
    const current = [...deps, tick];
    const quiet = lastDeps.current?.length === current.length && current.every((v, i) => Object.is(v, lastDeps.current?.[i]));
    lastDeps.current = current;
    const fresh = forced.current;
    forced.current = false;
    if (!quiet) setState((s) => ({ ...s, loading: true, error: undefined }));
    load(fresh).then(
      (data) => alive && setState({ data, loading: false }),
      (e: unknown) => alive && setState((s) => ({ ...s, loading: false, error: e instanceof Error ? e.message : String(e) })),
    );
    return () => {
      alive = false;
    };
  }, [...deps, tick, version]);
  const reload = () => {
    forced.current = true;
    setTick((t) => t + 1);
  };
  return { ...state, reload };
};

/** サーバーが外部の情報源から裏で読み直した値が変わるたびに増える数。スナップショットから受け取る。 */
export const CacheVersionContext = createContext(0);

/** サーバーのキャッシュを通す API を読む。サーバーが裏で読み直した値が変わったら、取り直して差し替える。 */
export const useCached = <T,>(load: (fresh: boolean) => Promise<T>, deps: unknown[]): FetchState<T> => useFetch(load, deps, useContext(CacheVersionContext));

let waiting = 0;

/** 待っている間は、画面のどこでもカーソルを矢印と砂時計にする。待ちが重なっても、すべて終わるまで戻さない。 */
export const useWaitCursor = (on: boolean): void => {
  useEffect(() => {
    if (!on) return;
    document.documentElement.classList.toggle('is-waiting', ++waiting > 0);
    return () => {
      document.documentElement.classList.toggle('is-waiting', --waiting > 0);
    };
  }, [on]);
};

/** セッションのウィジェットで選ぶよう頼まれたもの。ウィジェットの種類とセッションの id ごとに持つ。 */
const picked = new Map<string, string>();
const pickListeners = new Set<() => void>();

/** セッションのウィジェットに選べるものが複数あるとき、value を選ぶ。PR は URL で、JIRA の課題はキーで選ぶ。 */
export const pickInWidget = (kind: string, session: string, value: string): void => {
  picked.set(`${kind}\0${session}`, value);
  pickListeners.forEach((fn) => fn());
};

export const usePicked = (kind: string, session: string): string | undefined => {
  const key = `${kind}\0${session}`;
  const [pick, setPick] = useState(picked.get(key));
  useEffect(() => {
    const take = () => setPick(picked.get(key));
    take();
    pickListeners.add(take);
    return () => {
      pickListeners.delete(take);
    };
  }, [key]);
  return pick;
};

/**
 * 折りたたんだものの名前を key でブラウザに保存し、開き直しても閉じたままにする。toggle は details の toggle で呼ぶ。
 * details は開いた状態で描いたときにも toggle を送るので、開閉が変わったときだけ保存する。
 */
export const useCollapsed = (key: string): { collapsed: Set<string>; toggle: (name: string, open: boolean) => void } => {
  const [collapsed, setCollapsed] = useState(() => {
    try {
      return new Set(JSON.parse(localStorage.getItem(key) ?? '[]') as string[]);
    } catch {
      return new Set<string>();
    }
  });
  const toggle = (name: string, open: boolean) => {
    if (open === !collapsed.has(name)) return;
    const next = new Set(collapsed);
    if (open) next.delete(name);
    else next.add(name);
    setCollapsed(next);
    localStorage.setItem(key, JSON.stringify([...next]));
  };
  return { collapsed, toggle };
};
