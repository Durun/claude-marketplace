import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { RepoRef } from '../shared/types.ts';
import { candidates, typedAt } from '../client/completion.ts';

const ref = (number: number, title: string, mine = false, kind: RepoRef['kind'] = 'pr'): RepoRef => ({ number, title, kind, createdAt: 0, mine });
// サーバーから新しい順に届く。
const REFS = [ref(12, 'ログインの修正', true), ref(120, 'Fix login for v3'), ref(7, 'ログ画面を足す', true, 'issue'), ref(3, 'Update README')];
const labels = (word: string): string[] => candidates({ mark: '#', word }, [], REFS).map((c) => c.label);

test('typedAt は、行の頭か空白の後の / と # から、カーソルまでの語を返し、URL の途中や見出しの # では返さない', () => {
  assert.deepEqual(typedAt('see #12'), { mark: '#', word: '12' });
  assert.deepEqual(typedAt('#'), { mark: '#', word: '' });
  assert.deepEqual(typedAt('/rev'), { mark: '/', word: 'rev' });
  assert.equal(typedAt('https://example.test/a#b'), undefined);
  assert.equal(typedAt('# 見出し'), undefined);
});

test('candidates は、# の後に何も打っていなければ、自分が作った PR と Issue を新しい順に出す', () => {
  assert.deepEqual(candidates({ mark: '#', word: '' }, [], REFS), [
    { label: '#12', desc: 'PR · ログインの修正' },
    { label: '#7', desc: 'Issue · ログ画面を足す' },
  ]);
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
  assert.deepEqual(candidates({ mark: '/', word: 'rev' }, skills, []).map((c) => c.label), ['/review', '/plugin:reviewer', '/code-review']);
});
