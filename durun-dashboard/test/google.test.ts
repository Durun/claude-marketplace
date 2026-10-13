import assert from 'node:assert/strict';
import { test } from 'node:test';
import { bingResultsOf, googleResultsOf, oldGoogle } from '../server/google.ts';

const ck = (to: string) => `/ck/a?!&amp;&amp;p=abc&amp;u=a1${Buffer.from(to).toString('base64url')}&amp;ntb=1`;
const algo = (href: string, title: string, snippet: string) => `<li class="b_algo" data-id><script>x()</script><h2><a href="${href}" target="_blank">${title}</a></h2><p class="b_lineclamp2">${snippet}</p></li>`;

test('googleResultsOf は、題と要約を強調だけ残して取り出し、http(s) 以外の行き先を除き、Wikipedia は中継に向ける', () => {
  const items = [
    { link: 'https://example.com/a', htmlTitle: '<b>XP</b> の<span>話</span>', htmlSnippet: '2001 年 <b>XP</b> &amp; 95' },
    { link: 'javascript:alert(1)', htmlTitle: 'x', htmlSnippet: '' },
    { link: 'https://ja.wikipedia.org/wiki/XP', htmlTitle: 'W' },
  ];
  assert.deepEqual(googleResultsOf(items), [
    { href: 'https://example.com/a', display: 'example.com/a', title: '<b>XP</b> の話', snippet: '2001 年 <b>XP</b> &amp; 95' },
    { href: '/api/wikipedia?url=https%3A%2F%2Fja.wikipedia.org%2Fwiki%2FXP', display: 'ja.wikipedia.org/wiki/XP', title: 'W', snippet: '' },
  ]);
});

test('bingResultsOf は、リンクを計測用のリダイレクトから行き先そのものにし、http(s) 以外へのリダイレクトを除く', () => {
  const html = algo(`https://www.bing.com${ck('https://example.com/a?b="c"')}`, '<strong>XP</strong> の話', '<span class="d">2025年</span> <strong>XP</strong>') + algo(ck('javascript:alert(1)'), 'x', '');
  assert.deepEqual(bingResultsOf(html), [{ href: 'https://example.com/a?b=%22c%22', display: 'example.com/a?b="c"', title: '<b>XP</b> の話', snippet: '2025年 <b>XP</b>' }]);
});

test('oldGoogle は、件数と断り書きを書き、検索語を逃がし、ページ送りを start で中継に向ける', () => {
  const html = oldGoogle({ results: [{ href: 'https://example.com/', display: 'example.com/', title: 'E', snippet: '' }], count: '1,234', notice: 'Bing <です>' }, '?q=%22%3Cb%3E&start=10');
  assert.match(html, /value="&quot;&lt;b>"/);
  assert.match(html, /約 <b>1,234<\/b> 件中 <b>11<\/b> - <b>11<\/b> 件目/);
  assert.match(html, /<p class="notice">Bing &lt;です><\/p>/);
  assert.match(html, /<a href="\/api\/google\/search\?q=%22%3Cb%3E&amp;start=0">前へ<\/a>/);
  assert.match(html, /<b style="color:#a90a08">2<\/b>/);
});
