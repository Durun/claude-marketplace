import assert from 'node:assert/strict';
import { test } from 'node:test';
import { insertAt } from '../client/attach.ts';

test('カーソルの位置に入れ、前の語とは空白で区切り、後ろに空白を足す', () => {
  assert.deepEqual(insertAt('', 0, 'PROJ-1'), { value: 'PROJ-1 ', caret: 7 });
  assert.deepEqual(insertAt('見て', 2, 'a.ts'), { value: '見て a.ts ', caret: 8 });
  assert.deepEqual(insertAt('見て \n続き', 3, 'a.ts'), { value: '見て a.ts \n続き', caret: 8 });
});
