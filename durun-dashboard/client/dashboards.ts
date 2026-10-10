// ホームのダッシュボードは、名前を付けて複数の配置を保存し、メニューから切り替える。一覧と選んでいるものは、ブラウザに保存する。
import { useEffect, useState } from 'preact/hooks';

export type DashboardEntry = { id: string; name: string };
type State = { current: string; list: DashboardEntry[] };

/** ホームのウィジェットを浮かべるときの保存キー。浮かべたウィンドウは、ダッシュボードを切り替えても出したままにする。 */
export const HOME_KEY = 'dashboard-layout';
const STORAGE_KEY = 'dashboards';
const NEW_NAME = '新しいダッシュボード';
// 最初のダッシュボードは、複数持てるようになる前から HOME_KEY に保存していた配置をそのまま使う。
const FIRST: State = { current: '', list: [{ id: '', name: 'ダッシュボード' }] };

export const layoutKeyOf = (id: string): string => (id ? `${HOME_KEY}:${id}` : HOME_KEY);

/** names に無い名前にする。base が使われていれば「base (2)」「base (3)」と番号を付ける。 */
export const uniqueName = (names: string[], base: string): string => {
  let name = base;
  for (let n = 2; names.includes(name); n++) name = `${base} (${n})`;
  return name;
};

const load = (): State => {
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? 'null') as State | null;
    if (saved?.list.length) return saved;
  } catch {
    // 読めない一覧は捨てて、最初のダッシュボードだけにする。
  }
  return FIRST;
};

let state = load();
const listeners = new Set<(s: State) => void>();

const setState = (next: State): void => {
  state = next;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    // 保存できなくても、開いている間は切り替えられる。
  }
  listeners.forEach((fn) => fn(next));
};

export const useDashboards = (): State => {
  const [s, setS] = useState(state);
  useEffect(() => {
    listeners.add(setS);
    return () => {
      listeners.delete(setS);
    };
  }, []);
  return s;
};

const currentEntry = (): DashboardEntry => state.list.find((d) => d.id === state.current) ?? state.list[0]!;

export const switchDashboard = (id: string): void => setState({ ...state, current: id });

/** layout の配置で、name を元に重ならない名前のダッシュボードを足して選ぶ。layout が無ければ、初期配置になる。 */
const add = (name: string, layout: string | null): void => {
  const id = String(Date.now());
  try {
    if (layout) localStorage.setItem(layoutKeyOf(id), layout);
  } catch {
    // 保存できなければ、初期配置で作る。
  }
  setState({ current: id, list: [...state.list, { id, name: uniqueName(state.list.map((d) => d.name), name) }] });
};

/** ウィジェットを置いていないダッシュボードを作って選ぶ。 */
export const createDashboard = (): void => add(NEW_NAME, '[]');

/** 選んでいるダッシュボードの配置を写したものを作って選ぶ。 */
export const duplicateDashboard = (): void => {
  const src = currentEntry();
  add(`${src.name} のコピー`, localStorage.getItem(layoutKeyOf(src.id)));
};

/** 選んでいるダッシュボードの名前を変える。空の名前は受け付けない。 */
export const renameDashboard = (name: string): void => {
  const id = currentEntry().id;
  if (name.trim()) setState({ ...state, list: state.list.map((d) => (d.id === id ? { ...d, name: name.trim() } : d)) });
};
