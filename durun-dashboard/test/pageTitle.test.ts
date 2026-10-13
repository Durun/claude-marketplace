import assert from 'node:assert/strict';
import { test } from 'node:test';
import { findTitle, pageTitleOf } from '../server/pageTitle.ts';

test('findTitle は、最初の title の中身を、文字参照を戻して空白をまとめて返す', () => {
  assert.equal(findTitle('<head><TITLE lang="ja">\n  A &amp; B &#x2014; C&#39;s &nbsp;page\n</TITLE><title>x</title>'), "A & B — C's page");
  assert.equal(findTitle('<title>  </title>'), undefined);
  assert.equal(findTitle('<svg><text>no title</text></svg>'), undefined);
});

test('pageTitleOf は、Notion のページと claude.ai を読まない', async () => {
  assert.equal(await pageTitleOf(new URL('https://www.notion.so/team/Plan-0123456789abcdef0123456789abcdef')), undefined);
  assert.equal(await pageTitleOf(new URL('https://claude.ai/public/artifacts/abc')), undefined);
});
