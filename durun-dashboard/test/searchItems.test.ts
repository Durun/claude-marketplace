import assert from 'node:assert/strict';
import { test } from 'node:test';
import { localGroups, logGroup, refGroups } from '../client/searchItems.ts';
import type { InboxSection, JiraIssue, SearchResponse, Snapshot } from '../shared/types.ts';

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

test('JIRA の課題と PR は、セッションのバッジにあるものを先に、補完の課題と GitHub Inbox の PR を後に並べて同じものを 1 つにまとめ、何か打つまでは出さない', () => {
  const pr = (repo: string, number: number, title: string) => ({ url: `https://github.com/${repo}/pull/${number}`, repo, number, title, author: 'someone', updatedAt: 1 });
  const jira = (key: string, summary: string, status?: string) => ({ key, url: `https://jira.example/browse/${key}`, summary, status });
  const refs = {
    sessions: [
      session('a', 'favicon の保存先', 1, { jira: [jira('WEB-1', 'favicon を直す', '進行中')], prs: [pr('o/app', 1, 'favicon を直す')] }),
      session('b', 'CDN の設定', 2, { jira: [jira('WEB-2', 'favicon を CDN に載せる')] }),
    ],
  } as unknown as Snapshot;
  const issues: JiraIssue[] = [jira('WEB-1', 'favicon を直す', '完了'), jira('WEB-3', 'ログの整理')];
  const inbox: InboxSection[] = [{ title: 'Needs your review', prs: [pr('o/app', 1, 'favicon を直す'), pr('o/web', 9, 'Favicon の色')] }];
  const found = (q: string) => refGroups(refs, q, issues, inbox).map((g) => [g.title, g.items.map((i) => [i.label, i.sub, i.href])]);
  assert.deepEqual(found(''), []);
  assert.deepEqual(found('favicon'), [
    ['JIRA', [['WEB-2 favicon を CDN に載せる', undefined, 'https://jira.example/browse/WEB-2'], ['WEB-1 favicon を直す', '完了', 'https://jira.example/browse/WEB-1']]],
    ['PR', [['#1 favicon を直す', 'o/app', 'https://github.com/o/app/pull/1'], ['#9 Favicon の色', 'o/web', 'https://github.com/o/web/pull/9']]],
  ]);
  assert.deepEqual(found('FAVICON o/web'), [['PR', [['#9 Favicon の色', 'o/web', 'https://github.com/o/web/pull/9']]]]);
});
