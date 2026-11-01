import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { GithubItem, JiraIssue } from '../shared/types.ts';
import { GITHUB_GROUPINGS, JIRA_GROUPINGS, groupItems, groupingsFor } from '../client/listGroups.ts';

const issue = (key: string, fields: Partial<JiraIssue> = {}): JiraIssue => ({ key, url: '', ...fields });
const by = (key: string) => JIRA_GROUPINGS.find((g) => g.key === key)!;
const shape = (groups: { name: string; items: JiraIssue[] }[]) => groups.map((g) => [g.name, g.items.map((i) => i.key)]);

test('groupItems は、渡した順のまま同じ値が続くところを組にし、離れた同じ値は別の組にして、値の無い課題を none の名前の組にする', () => {
  const issues = [issue('CATS-1', { priority: 'Low' }), issue('CATS-2'), issue('CATS-3', { priority: 'Highest' }), issue('CATS-4', { priority: 'Low' })];
  assert.deepEqual(shape(groupItems(issues, by('priority'))), [['Low', ['CATS-1']], ['優先度なし', ['CATS-2']], ['Highest', ['CATS-3']], ['Low', ['CATS-4']]]);
});

test('groupItems は、エピックを親の課題のキーと題名で、プロジェクトを課題のキーで分ける', () => {
  const issues = [issue('CATS-2', { parent: { key: 'CATS-1', summary: '親' } }), issue('LPO-3'), issue('CATS-4')];
  assert.deepEqual(shape(groupItems(issues, by('epic'))), [['CATS-1 親', ['CATS-2']], ['エピックなし', ['LPO-3', 'CATS-4']]]);
  assert.deepEqual(shape(groupItems(issues, by('project'))), [['CATS', ['CATS-2']], ['LPO', ['LPO-3']], ['CATS', ['CATS-4']]]);
});

test('groupingsFor は、選んだ属性を、結果の並びでまとまっているものから外側に並べる', () => {
  // ステータス、プロジェクトの順に並べた結果。
  const issues = [
    issue('CATS-1', { status: '未着手', priority: 'Low' }),
    issue('LPO-2', { status: '未着手', priority: 'High' }),
    issue('CATS-3', { status: '進行中', priority: 'Low' }),
    issue('LPO-4', { status: '進行中', priority: 'High' }),
  ];
  const keys = (selected: string[], list = issues) => groupingsFor(JIRA_GROUPINGS, new Set(selected), list).map((g) => g.key);
  assert.deepEqual(keys(['project', 'status']), ['status', 'project']);
  assert.deepEqual(keys(['priority', 'project', 'status']), ['status', 'priority', 'project']);
  // どれもまとまっているときは、渡した属性の順に置く。
  assert.deepEqual(keys(['project', 'status'], [issue('CATS-1', { status: '未着手' })]), ['status', 'project']);
});

test('groupItems は、期限の組を「期限 YYYY-MM-DD」と名付ける', () => {
  const issues = [issue('CATS-1', { dueDate: '2026-11-15' }), issue('CATS-2')];
  assert.deepEqual(shape(groupItems(issues, by('dueDate'))), [['期限 2026-11-15', ['CATS-1']], ['期限なし', ['CATS-2']]]);
});

test('GitHub の項目は、Inbox の区分、レビューの判定、チェックの状態、状態と PR か Issue かを合わせたタイプを名前にし、値の無いものを none の名前の組にする', () => {
  const item = (number: number, over: Partial<GithubItem> = {}): GithubItem => ({ url: '', repo: 'o/r', number, title: '', author: 'me', updatedAt: 0, kind: 'pr', state: 'Open', ...over });
  const items = [item(1, { section: 'Needs your review', review: 'APPROVED', checks: 'FAILURE' }), item(2, { kind: 'issue' }), item(3, { state: 'Draft' })];
  const names = (key: string) => groupItems(items, GITHUB_GROUPINGS.find((g) => g.key === key)!).map((g) => g.name);
  assert.deepEqual(names('section'), ['Needs your review', 'Other']);
  assert.deepEqual(names('review'), ['承認済み', 'レビュー不要']);
  assert.deepEqual(names('checks'), ['チェック失敗', 'チェックなし']);
  assert.deepEqual(names('type'), ['Open PR', 'Open Issue', 'Draft PR']);
});

test('Inbox の区分は、いちばん内側の組にし、組の中でだけ Inbox の順に並べ替えて同じ区分をまとめる', () => {
  const item = (number: number, over: Partial<GithubItem> = {}): GithubItem => ({ url: '', repo: 'o/r', number, title: '', author: 'me', updatedAt: 0, kind: 'pr', state: 'Open', ...over });
  const items = [item(1, { section: 'Your drafts' }), item(2), item(3, { section: 'Needs your review' }), item(4, { section: 'Your drafts' })];
  const section = GITHUB_GROUPINGS.find((g) => g.key === 'section')!;
  assert.deepEqual(groupItems(items, section).map((g) => [g.name, g.items.map((i) => i.number)]), [['Needs your review', [3]], ['Your drafts', [1, 4]], ['Other', [2]]]);
  // 結果の並びでまとまっていても、ほかの属性より内側に置く。
  assert.deepEqual(groupingsFor(GITHUB_GROUPINGS, new Set(['section', 'author']), items).map((g) => g.key), ['author', 'section']);
});
