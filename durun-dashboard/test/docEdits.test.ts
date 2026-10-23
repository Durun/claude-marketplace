import assert from 'node:assert/strict';
import { test } from 'node:test';
import { type DocEdit, withDocEdits } from '../client/docEdits.ts';

test('withDocEdits は、スナップショットに載っていない操作を押した順に重ね、サーバーが済ませた後に作ったスナップショットには重ねない', () => {
  const state = { hiddenDocs: ['x'], pinnedDocs: ['a'], docTitles: { a: '付け直した題名' } };
  const edits: DocEdit[] = [
    { kind: 'pin', keys: ['b'], pinned: true, at: 100 },
    { kind: 'pin', keys: ['a'], pinned: false },
    { kind: 'rename', key: 'a', title: ' ' },
    { kind: 'rename', key: 'b', title: ' 新しい題名 ' },
    { kind: 'hide', keys: ['c', 'd'] },
  ];
  // 済ませた時刻と同じ時刻に作ったスナップショットは、済ませる前に作ったものかもしれない。
  assert.deepEqual(withDocEdits(state, edits, 100), { hiddenDocs: ['x', 'c', 'd'], pinnedDocs: ['b'], docTitles: { b: '新しい題名' } });
  assert.deepEqual(withDocEdits(state, edits, 101).pinnedDocs, []);
});
