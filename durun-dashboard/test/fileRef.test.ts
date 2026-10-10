import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { DiffFile } from '../shared/types.ts';
import { findFocus, isFileRef } from '../client/fileRef.ts';

test('ファイルを指すコードだけを見分け、相対パス、絶対パス、ファイル名だけの指定から差分のファイルと行を探す', () => {
  assert.deepEqual(['client/Home.tsx:32', 'README.md', 'server/main.ts:10-20', '.github/workflows/ci.yml', 'conf.d/app.json', '~/out/S1-3.mp4', 'shot.PNG'].map(isFileRef), [true, true, true, true, true, true, true]);
  assert.deepEqual(
    ['meta.title', 'pnpm check', 'example.com', '8eacda8', 'cdn.example-x.app/assets/js/main.js'].map(isFileRef),
    [false, false, false, false, false],
  );

  const files = ['client/Home.tsx', 'server/main.ts'].map((path) => ({ path }) as DiffFile);
  assert.deepEqual(findFocus(files, 'server/main.ts:160'), { index: 1, line: 160 });
  assert.deepEqual(findFocus(files, '/Users/me/repo/client/Home.tsx'), { index: 0, line: undefined });
  assert.deepEqual(findFocus(files, 'main.ts:3-5'), { index: 1, line: 3 });
  assert.equal(findFocus(files, 'server/haiku.ts'), undefined);
});
