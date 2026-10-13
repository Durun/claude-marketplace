import assert from 'node:assert/strict';
import { test } from 'node:test';
import { oldYahooSearch, topicsOf } from '../server/yahoo.ts';

test('topicsOf は、トピックスの記事のリンクの h1 を題にし、記事を中継に向ける', () => {
  const html = '<a href="https://news.yahoo.co.jp/pickup/1"><div><h1><span>題 &amp; 1</span></h1><span>NEW</span>12</div></a><a href="https://news.yahoo.co.jp/pickup/2"><span>h1 の無いリンク</span></a><a href="https://example.com/">よそ</a>';
  assert.deepEqual(topicsOf(html), [{ href: '/api/retro?url=https%3A%2F%2Fnews.yahoo.co.jp%2Fpickup%2F1', title: '題 &amp; 1' }]);
});

test('oldYahooSearch は、b から件数とページ送りを数え、検索語を逃がす', () => {
  const html = oldYahooSearch([{ href: 'https://example.com/', display: 'example.com/', title: 'E', snippet: '' }], '?p=%22%3Cx&b=11');
  assert.match(html, /value="&quot;&lt;x"/);
  assert.match(html, /<ol start="11">/);
  assert.match(html, /<a href="\/api\/yahoo\/search\?p=%22%3Cx&amp;b=1">前へ<\/a>/);
  assert.match(html, /<b>2<\/b>/);
});
