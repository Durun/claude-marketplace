import assert from 'node:assert/strict';
import { test } from 'node:test';
import { imgOf } from '../shared/html.ts';
import { firstHeading } from '../shared/markdown.ts';

test('firstHeading は、最初の見出しを返し、コードブロックの中の # とフロントマターは見出しとみなさない', () => {
  assert.equal(firstHeading('前書き\n\n## 設計メモ\n\n# 後の見出し\n'), '設計メモ');
  assert.equal(firstHeading('```sh\n# コメント\n```\n\n# 本当の見出し\n'), '本当の見出し');
  assert.equal(firstHeading('---\ntitle: x\n---\n\n# 題名\n'), '題名');
  assert.equal(firstHeading('下線の見出し\n===\n'), '下線の見出し');
  assert.equal(firstHeading('見出しの無い文\n'), undefined);
});

test('imgOf は、<img> を 1 つだけ書いた HTML から src と alt を読み、ほかの HTML は読まない', () => {
  const src = 'https://github.com/user-attachments/assets/00000000-0000-0000-0000-000000000000';
  assert.deepEqual(imgOf(`<img width="1707" height="995" alt="image" src="${src}" />\n`), { src, alt: 'image' });
  assert.deepEqual(imgOf("<img data-src='a.png' src='b.png'>"), { src: 'b.png', alt: '' });
  assert.equal(imgOf(`<a href="${src}"><img src="${src}"></a>`), undefined);
  assert.equal(imgOf('<img src="a.png"><img src="b.png">'), undefined);
  assert.equal(imgOf('<img alt="src の無い画像">'), undefined);
});
