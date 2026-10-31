import { useEffect, useState } from 'preact/hooks';
import type { Attention } from '../shared/types.ts';

export type SessionTab = 'overview' | 'code' | 'usage';

export type Route =
  | { page: 'home' }
  /** file はコードの画面で開くファイル。`client/Home.tsx:32` のように行を付けてもよい。 */
  | { page: 'session'; id: string; tab: SessionTab; file?: string }
  | { page: 'docs' }
  | { page: 'doc'; id: string }
  | { page: 'search'; q: string };

const TAB_SEGMENTS: Record<string, SessionTab> = { code: 'code', usage: 'usage' };

export const safeDecode = (s: string): string => {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
};

const parseHash = (hash: string): Route => {
  const [head, id, tab, file] = hash.replace(/^#\/?/, '').split('/').filter(Boolean).map(safeDecode);
  if (head === 's' && id) return { page: 'session', id, tab: (tab && TAB_SEGMENTS[tab]) || 'overview', file };
  if (head === 'doc' && id) return { page: 'doc', id };
  if (head === 'docs') return { page: 'docs' };
  if (head === 'search') return { page: 'search', q: id ?? '' };
  return { page: 'home' };
};

/** 開いている画面のセッションの id。セッションの画面でなければ空。 */
export const currentSessionId = (): string => {
  const route = parseHash(location.hash);
  return route.page === 'session' ? route.id : '';
};

/** 開いている画面の種類。id と検索語は含めないので、同じ種類の画面で起きたことをまとめられる。 */
export const currentPage = (): string => {
  const route = parseHash(location.hash);
  return route.page === 'session' ? `session/${route.tab}` : route.page;
};

export const href = {
  home: (): string => '#/',
  docs: (): string => '#/docs',
  doc: (id: string): string => `#/doc/${encodeURIComponent(id)}`,
  search: (q = ''): string => (q ? `#/search/${encodeURIComponent(q)}` : '#/search'),
  session: (id: string, tab: SessionTab = 'overview'): string =>
    `#/s/${encodeURIComponent(id)}${tab === 'overview' ? '' : `/${tab}`}`,
  file: (id: string, file: string): string => `${href.session(id, 'code')}/${encodeURIComponent(file)}`,
};

/** 通知やボタンから、その項目に対応できる画面へ移るための遷移先。 */
export const attentionHref = (a: Attention): string => {
  if (a.kind === 'review') return href.session(a.sessionId, 'code');
  if (a.kind === 'doc' && a.docId) return href.doc(a.docId);
  if (a.kind === 'reply' || a.kind === 'pr' || a.kind === 'command') return href.session(a.sessionId);
  return href.home();
};

export const useRoute = (): Route => {
  const [route, setRoute] = useState<Route>(() => parseHash(location.hash));
  useEffect(() => {
    const onChange = () => {
      setRoute(parseHash(location.hash));
      window.scrollTo(0, 0);
    };
    window.addEventListener('hashchange', onChange);
    return () => window.removeEventListener('hashchange', onChange);
  }, []);
  return route;
};
