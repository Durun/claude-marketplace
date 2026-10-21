// 検索窓（Cmd/Ctrl + K）に並べる候補。画面にあるデータはその場で語に当たるものに絞り、会話ログはサーバーの検索の結果から作る。
import type { SearchResponse, SessionStatus, Snapshot, WaitingFor } from '../shared/types.ts';
import { dateTime } from './format.ts';
import { KIND_LABEL } from './notify.ts';
import { attentionHref, href } from './router.ts';

const MAX_PER_GROUP = 5;

export type SearchItem = { key: string; label: string; sub?: string; href: string; status?: SessionStatus; waitingFor?: WaitingFor };
/** note は、見出しの横に出す進み具合や失敗の理由。 */
export type SearchGroup = { title: string; items: SearchItem[]; note?: string };

/** 会話ログの検索と同じく、空白で区切った語をすべて含むものに絞る。大文字と小文字は区別しない。 */
export const matcher = (q: string) => {
  const terms = q.toLowerCase().split(/\s+/).filter(Boolean);
  return (...texts: (string | undefined)[]): boolean => {
    const text = texts.join('\n').toLowerCase();
    return terms.every((t) => text.includes(t));
  };
};

const joined = (...parts: (string | undefined)[]): string => parts.filter(Boolean).join(' · ');

const PAGES: SearchItem[] = [
  { key: 'page:home', label: 'ホーム', href: href.home() },
  { key: 'page:docs', label: 'すべての資料', href: href.docs() },
  { key: 'page:search', label: '会話ログの検索', href: href.search() },
];

/** 語に当たるものを、種類ごとに新しい順に並べる。何も打っていなければ、すべてが当たる。 */
export const localGroups = (snapshot: Snapshot | null, q: string): SearchGroup[] => {
  const hit = matcher(q);
  const sessions = snapshot?.sessions ?? [];
  const names = new Map(sessions.map((s) => [s.id, s.name]));
  const groups: SearchGroup[] = [
    {
      title: 'セッション',
      items: sessions
        .filter((s) => hit(s.name, s.theme, s.repo, s.branch, s.cwd, s.pr && `#${s.pr.number} ${s.pr.title}`, ...s.jira.map((j) => `${j.key} ${j.summary ?? ''}`)))
        .toSorted((a, b) => b.updatedAt - a.updatedAt)
        .map((s) => ({ key: `session:${s.id}`, label: s.name, sub: joined(s.repo, s.branch), href: href.session(s.id), status: s.status, waitingFor: s.waitingFor })),
    },
    {
      title: '要対応',
      // 未読の資料は、資料の候補に出す。
      items: (snapshot?.attention ?? [])
        .filter((a) => a.kind !== 'doc' && hit(a.title, a.body, names.get(a.sessionId)))
        .toSorted((a, b) => b.createdAt - a.createdAt)
        .map((a) => ({ key: `attention:${a.id}`, label: a.title, sub: joined(KIND_LABEL[a.kind], names.get(a.sessionId)), href: attentionHref(a) })),
    },
    {
      title: '資料',
      items: (snapshot?.docs ?? [])
        .filter((d) => hit(d.title, names.get(d.sessionId)))
        .toSorted((a, b) => b.createdAt - a.createdAt)
        .map((d) => ({ key: `doc:${d.id}`, label: d.title, sub: joined(names.get(d.sessionId), dateTime(d.createdAt)), href: href.doc(d.id) })),
    },
    { title: '画面', items: PAGES.filter((p) => hit(p.label)) },
  ];
  return groups.map((g) => ({ ...g, items: g.items.slice(0, MAX_PER_GROUP) })).filter((g) => g.items.length > 0);
};

/**
 * 会話ログで当たったセッション。ダッシュボードの一覧に無いセッションは、再開するコマンドを出せる検索の画面へ送る。
 * 最後に、検索の画面ですべての結果を見る候補を置く。
 */
export const logGroup = (snapshot: Snapshot | null, q: string, res: SearchResponse | undefined): SearchGroup => {
  const names = new Map(snapshot?.sessions.map((s) => [s.id, s.name]));
  const items = (res?.results ?? []).slice(0, MAX_PER_GROUP).map((r) => ({
    key: `log:${r.id}`,
    label: names.get(r.id) ?? r.title,
    // 題名は label に出しているので、依頼か発言で当たった箇所を添える。
    sub: r.hits.find((h) => h.kind !== 'title')?.text,
    href: names.has(r.id) ? href.session(r.id) : href.search(q),
  }));
  return { title: '会話ログ', items: [...items, { key: 'log:all', label: `「${q}」をすべての会話ログから探す`, href: href.search(q) }] };
};
