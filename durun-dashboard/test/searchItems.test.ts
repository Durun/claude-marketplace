import assert from 'node:assert/strict';
import { test } from 'node:test';
import { localGroups, logGroup } from '../client/searchItems.ts';
import type { SearchResponse, Snapshot } from '../shared/types.ts';

const session = (id: string, name: string, updatedAt: number, extra = {}) => ({ id, name, theme: '', repo: 'app', cwd: `/repo/${id}`, status: 'idle', updatedAt, jira: [], ...extra });

const snapshot = {
  sessions: [
    session('a', 'favicon の保存先', 1),
    session('b', 'CDN の設定', 2, { pr: { number: 7655, title: 'Favicon を CDN に載せる' } }),
    session('c', 'ログの整理', 3),
  ],
  attention: [
    { id: 'reply:a', kind: 'reply', sessionId: 'a', title: '入力待ち', body: 'favicon は CDN に置きます', createdAt: 5 },
    { id: 'doc:d', kind: 'doc', sessionId: 'a', title: '資料: favicon と CDN の図', docId: 'd', createdAt: 6 },
  ],
  docs: [{ id: 'd', sessionId: 'a', title: 'favicon と CDN の図', format: 'markdown', createdAt: 4, seen: false }],
} as unknown as Snapshot;

const hrefs = (q: string) => localGroups(snapshot, q).map((g) => [g.title, g.items.map((i) => i.href)]);

test('検索窓の候補は、空白で区切った語をすべて含むものに大文字と小文字を区別せずに絞り、種類ごとに新しい順に並べる', () => {
  assert.deepEqual(hrefs('FAVICON cdn'), [
    ['セッション', ['#/s/b']],
    ['要対応', ['#/s/a']],
    ['資料', ['#/doc/d']],
  ]);
  assert.deepEqual(hrefs('')[0], ['セッション', ['#/s/c', '#/s/b', '#/s/a']]);
});

test('会話ログで当たったセッションは、一覧にあれば概要へ、無ければ検索の画面へ送り、最後にすべてを探す候補を置く', () => {
  const res = {
    total: 2,
    results: [
      { id: 'a', cwd: '', repo: 'app', title: '題名', updatedAt: 2, hitCount: 2, hits: [{ kind: 'title', text: '題名' }, { kind: 'message', text: '…当たった発言…' }] },
      { id: 'old', cwd: '', repo: 'app', title: '古いセッション', updatedAt: 1, hitCount: 1, hits: [{ kind: 'prompt', text: '依頼' }] },
    ],
  } as SearchResponse;
  assert.deepEqual(logGroup(snapshot, 'favicon', res).items.map((i) => [i.label, i.sub, i.href]), [
    ['favicon の保存先', '…当たった発言…', '#/s/a'],
    ['古いセッション', '依頼', '#/search/favicon'],
    ['「favicon」をすべての会話ログから探す', undefined, '#/search/favicon'],
  ]);
});
