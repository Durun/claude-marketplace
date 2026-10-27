import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { IssueInfo, JiraIssue, PrInfo, RepoRef } from '../shared/types.ts';
import { candidates, typedAt } from '../client/completion.ts';
import { type GhRefs, toRefs } from '../server/integrations.ts';

const REPO = 'https://github.test/o/r';
const ref = (number: number, title: string, mine = false, kind: RepoRef['kind'] = 'pr', state: RepoRef['state'] = 'Open'): RepoRef => ({ number, title, url: `${REPO}/${kind === 'pr' ? 'pull' : 'issues'}/${number}`, kind, state, createdAt: 0, mine });
// サーバーから新しい順に届く。
const REFS = [ref(12, 'ログインの修正', true), ref(120, 'Fix login for v3'), ref(7, 'ログ画面を足す', true, 'issue', 'Closed'), ref(3, 'Update README')];
const labels = (word: string): string[] => candidates({ mark: '#', word }, [], REFS, [], []).map((c) => c.label);
const issue = (key: string, summary: string, status = '進行中'): JiraIssue => ({ key, url: `https://jira.test/browse/${key}`, summary, status });
// サーバーから、スペースごとに自分の課題、ほかの課題の順に、それぞれ更新が新しい順で届く。
const ISSUES = [issue('CATS-12', 'ログインの修正'), issue('CATS-3', '画面を足す', '完了'), issue('LPO-1', 'ログを見る'), issue('CATS-120', 'ログ画面の修正', '未着手')];
const issueLabels = (mark: string, word: string): string[] => candidates({ mark, word }, [], [], ISSUES, []).map((c) => c.label);

test('typedAt は、行の頭か空白の後の /、#、@ から、カーソルまでの語を返し、URL の途中や見出しの #、メールアドレスの @ では返さない', () => {
  assert.deepEqual(typedAt('see #12'), { mark: '#', word: '12' });
  assert.deepEqual(typedAt('#'), { mark: '#', word: '' });
  assert.deepEqual(typedAt('/rev'), { mark: '/', word: 'rev' });
  assert.deepEqual(typedAt('見て @client/Ho'), { mark: '@', word: 'client/Ho' });
  assert.equal(typedAt('https://example.test/a#b'), undefined);
  assert.equal(typedAt('# 見出し'), undefined);
  assert.equal(typedAt('me@example.test'), undefined);
});

test('typedAt は、英数字の後ろでなければ、JIRA のプロジェクトキーとハイフンから、カーソルまでの語を返す', () => {
  assert.deepEqual(typedAt('see CATS-12'), { mark: 'CATS-', word: '12' });
  assert.deepEqual(typedAt('次はlpo-'), { mark: 'lpo-', word: '' });
  assert.deepEqual(typedAt('#CATS-1'), { mark: '#', word: 'CATS-1' });
  assert.equal(typedAt('2CATS-'), undefined);
});

test('candidates は、# の後に何も打っていなければ、自分が作った PR と Issue を、種別と状態を添えて新しい順に出す', () => {
  assert.deepEqual(candidates({ mark: '#', word: '' }, [], REFS, [], []), [
    { label: '#12', desc: 'ログインの修正', icon: { kind: 'pr', state: 'Open' }, badge: false },
    { label: '#7', desc: 'ログ画面を足す', icon: { kind: 'issue', state: 'Closed' }, badge: false },
  ]);
});

const pr = (number: number, title: string, more: Partial<PrInfo> = {}): PrInfo => ({ url: `${REPO}/pull/${number}`, number, title, state: 'OPEN', isDraft: false, review: '', checks: { passed: 0, failed: 0, pending: 0 }, ...more });
const ghIssue = (number: number, title: string): IssueInfo => ({ url: `${REPO}/issues/${number}`, number, title, state: 'Open' });
// #120 は届いた PR と Issue にあり、#5 と #40 は無い。#9 はほかのリポジトリの PR。
const BADGES = { prs: [pr(120, 'Fix login for v3'), pr(5, '古い PR', { isDraft: true }), pr(9, 'ほかのリポジトリ', { url: 'https://github.test/o/other/pull/9' })], issues: [ghIssue(40, '古い Issue')], jira: [] };
const marked = (word: string): string[] => candidates({ mark: '#', word }, [], REFS, [], [], BADGES).map((c) => `${c.label} ${c.icon?.state} ${c.badge}`);

test('candidates は、# の後に何も打っていなければ、セッションのバッジの PR と Issue を、届いた中に無くても、自分が作ったものより先に出して印を付ける。ほかのリポジトリのものは出さない', () => {
  assert.deepEqual(marked(''), ['#120 Open true', '#5 Draft true', '#40 Open true', '#12 Open false', '#7 Closed false']);
});

test('candidates は、# の後に打った語に同じくらい合う候補の中では、セッションのバッジの PR と Issue を先に出す', () => {
  assert.deepEqual(marked('12'), ['#120 Open true', '#12 Open false']);
  assert.deepEqual(marked('古い'), ['#5 Draft true', '#40 Open true']);
  assert.deepEqual(marked('9'), []);
});

test('toRefs は、PR の状態を Draft、Open、Merged、Closed に、Issue の状態を Open、Closed、Canceled に分ける', () => {
  type Node = GhRefs['repository']['issues']['nodes'][number];
  const node = (number: number, state: Node['state'], more: Partial<Node> = {}): Node => ({ number, title: '', url: '', createdAt: new Date(number).toISOString(), state, ...more });
  const refs = toRefs({
    mine: { nodes: [] },
    repository: {
      pullRequests: { nodes: [node(1, 'OPEN', { isDraft: true }), node(2, 'OPEN', { isDraft: false }), node(3, 'MERGED'), node(4, 'CLOSED', { isDraft: true })] },
      issues: { nodes: [node(5, 'OPEN'), node(6, 'CLOSED', { stateReason: 'COMPLETED' }), node(7, 'CLOSED', { stateReason: null }), node(8, 'CLOSED', { stateReason: 'NOT_PLANNED' }), node(9, 'CLOSED', { stateReason: 'DUPLICATE' })] },
    },
  });
  assert.deepEqual(
    refs.map((r) => `${r.number} ${r.kind} ${r.state}`),
    ['9 issue Canceled', '8 issue Canceled', '7 issue Closed', '6 issue Closed', '5 issue Open', '4 pr Closed', '3 pr Merged', '2 pr Open', '1 pr Draft'],
  );
});

test('candidates は、# の後に打った語に、番号の頭、題名の一部、題名に順に現れる文字の順で合うものを出す', () => {
  assert.deepEqual(labels('12'), ['#12', '#120']);
  assert.deepEqual(labels('3'), ['#3', '#120']);
  assert.deepEqual(labels('LOG'), ['#120']);
  assert.deepEqual(labels('ログ'), ['#12', '#7']);
  assert.deepEqual(labels('ログ修正'), ['#12']);
  assert.deepEqual(labels('xyz'), []);
});

test('candidates は、/ の後に打った語に合う skills を、名前の頭が合うものから出す', () => {
  const skills = [{ name: 'code-review', description: 'a' }, { name: 'review', description: 'b' }, { name: 'plugin:reviewer', description: 'c' }];
  assert.deepEqual(candidates({ mark: '/', word: 'rev' }, skills, [], [], []).map((c) => c.label), ['/review', '/plugin:reviewer', '/code-review']);
});

test('candidates は、JIRA のプロジェクトキーとハイフンの後に何も打っていなければ、そのスペースの課題を、状態を添えて届いた順に出す', () => {
  assert.deepEqual(candidates({ mark: 'cats-', word: '' }, [], [], ISSUES, []), [
    { label: 'CATS-12', desc: '進行中 · ログインの修正', badge: false },
    { label: 'CATS-3', desc: '完了 · 画面を足す', badge: false },
    { label: 'CATS-120', desc: '未着手 · ログ画面の修正', badge: false },
  ]);
  assert.deepEqual(issueLabels('XYZ-', ''), []);
});

test('candidates は、JIRA のプロジェクトキーとハイフンの後に打った語に、番号の頭、題名の一部、題名に順に現れる文字の順で合う課題を出す', () => {
  assert.deepEqual(issueLabels('CATS-', '12'), ['CATS-12', 'CATS-120']);
  assert.deepEqual(issueLabels('CATS-', 'ログ'), ['CATS-12', 'CATS-120']);
  assert.deepEqual(issueLabels('CATS-', '画修'), ['CATS-120']);
});

test('candidates は、JIRA のプロジェクトキーとハイフンの後に何も打っていなければ、そのスペースの、セッションのバッジの課題を、届いた中に無くても先に出して印を付ける', () => {
  const jira = [issue('CATS-120', 'ログ画面の修正', '未着手'), issue('CATS-9', '古い課題', '完了'), issue('LPO-1', 'ログを見る')];
  const marks = candidates({ mark: 'CATS-', word: '' }, [], [], ISSUES, [], { prs: [], jira }).map((c) => `${c.label} ${c.badge}`);
  assert.deepEqual(marks, ['CATS-120 true', 'CATS-9 true', 'CATS-12 false', 'CATS-3 false']);
});
