import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { GithubItem, JiraIssue } from '../shared/types.ts';
import { GITHUB_GROUPINGS, JIRA_GROUPINGS, groupItems } from '../client/listGroups.ts';

const issue = (key: string, fields: Partial<JiraIssue> = {}): JiraIssue => ({ key, url: '', ...fields });
const by = (key: string) => JIRA_GROUPINGS.find((g) => g.key === key)!;
const shape = (groups: { name: string; items: JiraIssue[] }[]) => groups.map((g) => [g.name, g.items.map((i) => i.key)]);

test('groupItems は、同じ値の課題を、離れていても 1 つの組にまとめ、組を値が先に出た順に並べ、値の無い課題を none の名前の組にする', () => {
  const issues = [issue('CATS-1', { priority: 'Low' }), issue('CATS-2'), issue('CATS-3', { priority: 'Highest' }), issue('CATS-4', { priority: 'Low' })];
  assert.deepEqual(shape(groupItems(issues, by('priority'))), [['Low', ['CATS-1', 'CATS-4']], ['優先度なし', ['CATS-2']], ['Highest', ['CATS-3']]]);
});

test('groupItems は、エピックを親の課題のキーと題名で、プロジェクトを課題のキーで分ける', () => {
  const issues = [issue('CATS-2', { parent: { key: 'CATS-1', summary: '親' } }), issue('LPO-3'), issue('CATS-4')];
  assert.deepEqual(shape(groupItems(issues, by('epic'))), [['CATS-1 親', ['CATS-2']], ['エピックなし', ['LPO-3', 'CATS-4']]]);
  assert.deepEqual(shape(groupItems(issues, by('project'))), [['CATS', ['CATS-2', 'CATS-4']], ['LPO', ['LPO-3']]]);
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

test('Inbox の区分は、組を Inbox の順に並べ、どれにも入らない PR を最後に置く', () => {
  const item = (number: number, over: Partial<GithubItem> = {}): GithubItem => ({ url: '', repo: 'o/r', number, title: '', author: 'me', updatedAt: 0, kind: 'pr', state: 'Open', ...over });
  const items = [item(1, { section: 'Your drafts' }), item(2), item(3, { section: 'Needs your review' }), item(4, { section: 'Your drafts' })];
  const section = GITHUB_GROUPINGS.find((g) => g.key === 'section')!;
  assert.deepEqual(groupItems(items, section).map((g) => [g.name, g.items.map((i) => i.number)]), [['Needs your review', [3]], ['Your drafts', [1, 4]], ['Other', [2]]]);
});
