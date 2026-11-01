import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { JiraIssue } from '../shared/types.ts';
import { BY_PROJECT, GROUPINGS, groupIssues } from '../client/jiraGroups.ts';

const issue = (key: string, fields: Partial<JiraIssue> = {}): JiraIssue => ({ key, url: '', ...fields });
const by = (key: string) => GROUPINGS.find((g) => g.key === key)!;
const shape = (groups: ReturnType<typeof groupIssues>) => groups.map((g) => [g.name, g.issues.map((i) => i.key)]);

test('groupIssues は、値の無い課題の組を最後に置き、組の中は渡した順のまま並べる', () => {
  const issues = [issue('CATS-1'), issue('CATS-2', { assignee: '山田' }), issue('CATS-3', { assignee: '佐藤' }), issue('CATS-4', { assignee: '山田' })];
  assert.deepEqual(shape(groupIssues(issues, by('assignee'))), [['山田', ['CATS-2', 'CATS-4']], ['佐藤', ['CATS-3']], ['未割り当て', ['CATS-1']]]);
});

test('groupIssues は、ステータスを作業の進む順に、優先度を高い順に、期限を早い順に並べる', () => {
  const statuses = [issue('CATS-1', { status: '完了', statusCategory: 'done' }), issue('CATS-2', { status: 'レビュー中', statusCategory: 'indeterminate' }), issue('CATS-3', { status: '未着手', statusCategory: 'new' })];
  assert.deepEqual(shape(groupIssues(statuses, by('status'))).map(([name]) => name), ['未着手', 'レビュー中', '完了']);
  const priorities = [issue('CATS-1', { priority: '独自' }), issue('CATS-2', { priority: 'Low' }), issue('CATS-3'), issue('CATS-4', { priority: 'Highest' })];
  assert.deepEqual(shape(groupIssues(priorities, by('priority'))).map(([name]) => name), ['Highest', 'Low', '独自', '優先度なし']);
  const dues = [issue('CATS-1', { dueDate: '2026-12-01' }), issue('CATS-2'), issue('CATS-3', { dueDate: '2026-11-15' })];
  assert.deepEqual(shape(groupIssues(dues, by('dueDate'))).map(([name]) => name), ['2026-11-15', '2026-12-01', '期限なし']);
});

test('groupIssues は、エピックを親の課題のキーと題名で、プロジェクトを課題のキーで分ける', () => {
  const issues = [issue('CATS-2', { parent: { key: 'CATS-1', summary: '親' } }), issue('LPO-3'), issue('CATS-4')];
  assert.deepEqual(shape(groupIssues(issues, by('epic'))), [['CATS-1 親', ['CATS-2']], ['エピックなし', ['LPO-3', 'CATS-4']]]);
  assert.deepEqual(shape(groupIssues(issues, BY_PROJECT)), [['CATS', ['CATS-2', 'CATS-4']], ['LPO', ['LPO-3']]]);
});
