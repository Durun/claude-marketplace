import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { JiraIssue } from '../shared/types.ts';
import { GROUPINGS, groupIssues } from '../client/jiraGroups.ts';

const issue = (key: string, fields: Partial<JiraIssue> = {}): JiraIssue => ({ key, url: '', ...fields });
const by = (key: string) => GROUPINGS.find((g) => g.key === key)!;
const shape = (groups: ReturnType<typeof groupIssues>) => groups.map((g) => [g.name, g.issues.map((i) => i.key)]);

test('groupIssues は、組も組の中の課題も、渡した順で先に出たものから並べ、値の無い課題を none の名前の組にまとめる', () => {
  const issues = [issue('CATS-1', { priority: 'Low' }), issue('CATS-2'), issue('CATS-3', { priority: 'Highest' }), issue('CATS-4', { priority: 'Low' })];
  assert.deepEqual(shape(groupIssues(issues, by('priority'))), [['Low', ['CATS-1', 'CATS-4']], ['優先度なし', ['CATS-2']], ['Highest', ['CATS-3']]]);
});

test('groupIssues は、エピックを親の課題のキーと題名で、プロジェクトを課題のキーで分ける', () => {
  const issues = [issue('CATS-2', { parent: { key: 'CATS-1', summary: '親' } }), issue('LPO-3'), issue('CATS-4')];
  assert.deepEqual(shape(groupIssues(issues, by('epic'))), [['CATS-1 親', ['CATS-2']], ['エピックなし', ['LPO-3', 'CATS-4']]]);
  assert.deepEqual(shape(groupIssues(issues, by('project'))), [['CATS', ['CATS-2', 'CATS-4']], ['LPO', ['LPO-3']]]);
});
