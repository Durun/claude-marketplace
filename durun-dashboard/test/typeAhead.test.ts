import assert from 'node:assert/strict';
import { test } from 'node:test';
import { nextMatch } from '../client/typeAhead.ts';

const names = ['apps', 'Docs', 'docker', 'README.md', 'dist'];

test('nextMatch は、打った文字で始まる名前へ移り、文字を打ち足しても合う間は今の名前に留まる', () => {
  assert.equal(nextMatch(names, 'r', -1), 3);
  assert.equal(nextMatch(names, 'doc', 1), 1);
  assert.equal(nextMatch(names, 'dock', 1), 2);
  assert.equal(nextMatch(names, 'x', 0), -1);
});

test('nextMatch は、同じ 1 文字を重ねて打つと、その文字で始まる名前を順に巡り、末尾から先頭に戻る', () => {
  assert.equal(nextMatch(names, 'd', 1), 2);
  assert.equal(nextMatch(names, 'dd', 2), 4);
  assert.equal(nextMatch(names, 'ddd', 4), 1);
});
