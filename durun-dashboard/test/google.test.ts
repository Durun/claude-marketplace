import assert from 'node:assert/strict';
import { test } from 'node:test';
import { oldGoogle } from '../server/google.ts';

test('oldGoogle は、件数を数え、検索語を逃がし、ページ送りを start で中継に向ける', () => {
  const html = oldGoogle([{ href: 'https://example.com/', display: 'example.com/', title: 'E', snippet: '' }], '?q=%22%3Cb%3E&start=10');
  assert.match(html, /value="&quot;&lt;b>"/);
  assert.match(html, /<b>11<\/b> - <b>11<\/b> 件目/);
  assert.match(html, /<a href="\/api\/google\/search\?q=%22%3Cb%3E&amp;start=0">前へ<\/a>/);
  assert.match(html, /<b style="color:#a90a08">2<\/b>/);
});
