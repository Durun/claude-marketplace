import assert from 'node:assert/strict';
import { test } from 'node:test';
import { firstHeading } from '../shared/markdown.ts';

test('firstHeading は、最初の見出しを返し、コードブロックの中の # とフロントマターは見出しとみなさない', () => {
  assert.equal(firstHeading('前書き\n\n## 設計メモ\n\n# 後の見出し\n'), '設計メモ');
  assert.equal(firstHeading('```sh\n# コメント\n```\n\n# 本当の見出し\n'), '本当の見出し');
  assert.equal(firstHeading('---\ntitle: x\n---\n\n# 題名\n'), '題名');
  assert.equal(firstHeading('下線の見出し\n===\n'), '下線の見出し');
  assert.equal(firstHeading('見出しの無い文\n'), undefined);
});
