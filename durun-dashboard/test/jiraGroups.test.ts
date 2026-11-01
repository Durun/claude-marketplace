import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { JiraIssue } from '../shared/types.ts';
import { GROUPINGS, groupIssues, groupingsFor } from '../client/jiraGroups.ts';

const issue = (key: string, fields: Partial<JiraIssue> = {}): JiraIssue => ({ key, url: '', ...fields });
const by = (key: string) => GROUPINGS.find((g) => g.key === key)!;
const shape = (groups: ReturnType<typeof groupIssues>) => groups.map((g) => [g.name, g.issues.map((i) => i.key)]);

test('groupIssues は、渡した順のまま同じ値が続くところを組にし、離れた同じ値は別の組にして、値の無い課題を none の名前の組にする', () => {
  const issues = [issue('CATS-1', { priority: 'Low' }), issue('CATS-2'), issue('CATS-3', { priority: 'Highest' }), issue('CATS-4', { priority: 'Low' })];
  assert.deepEqual(shape(groupIssues(issues, by('priority'))), [['Low', ['CATS-1']], ['優先度なし', ['CATS-2']], ['Highest', ['CATS-3']], ['Low', ['CATS-4']]]);
});

test('groupIssues は、エピックを親の課題のキーと題名で、プロジェクトを課題のキーで分ける', () => {
  const issues = [issue('CATS-2', { parent: { key: 'CATS-1', summary: '親' } }), issue('LPO-3'), issue('CATS-4')];
  assert.deepEqual(shape(groupIssues(issues, by('epic'))), [['CATS-1 親', ['CATS-2']], ['エピックなし', ['LPO-3', 'CATS-4']]]);
  assert.deepEqual(shape(groupIssues(issues, by('project'))), [['CATS', ['CATS-2']], ['LPO', ['LPO-3']], ['CATS', ['CATS-4']]]);
});

test('groupingsFor は、選んだ属性を、結果の並びでまとまっているものから外側に並べる', () => {
  // ステータス、プロジェクトの順に並べた結果。
  const issues = [
    issue('CATS-1', { status: '未着手', priority: 'Low' }),
    issue('LPO-2', { status: '未着手', priority: 'High' }),
    issue('CATS-3', { status: '進行中', priority: 'Low' }),
    issue('LPO-4', { status: '進行中', priority: 'High' }),
  ];
  const keys = (selected: string[], list = issues) => groupingsFor(new Set(selected), list).map((g) => g.key);
  assert.deepEqual(keys(['project', 'status']), ['status', 'project']);
  assert.deepEqual(keys(['priority', 'project', 'status']), ['status', 'priority', 'project']);
  // どれもまとまっているときは、GROUPINGS の順に置く。
  assert.deepEqual(keys(['project', 'status'], [issue('CATS-1', { status: '未着手' })]), ['status', 'project']);
});
