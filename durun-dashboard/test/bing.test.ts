import assert from 'node:assert/strict';
import { test } from 'node:test';
import { luckyOf, oldGoogle } from '../server/bing.ts';

const ck = (to: string) => `/ck/a?!&amp;&amp;p=abc&amp;u=a1${Buffer.from(to).toString('base64url')}&amp;ntb=1`;
const algo = (href: string, title: string, snippet: string) => `<li class="b_algo" data-id><script>x()</script><h2><a href="${href}" target="_blank">${title}</a></h2><p class="b_lineclamp2">${snippet}</p></li>`;

test('oldGoogle は、結果の題と要約を強調だけ残して抜き出し、リンクを計測用のリダイレクトから行き先そのものにする', () => {
  const html = oldGoogle(algo(`https://www.bing.com${ck('https://example.com/a?b="c"')}`, '<strong>XP</strong> の話', '<span class="d">2025年</span> <strong>XP</strong> &amp; 95'), '?q=xp');
  assert.match(html, /<p class="g"><a href="https:\/\/example\.com\/a\?b=%22c%22"><b>XP<\/b> の話<\/a><br>2025年 <b>XP<\/b> &amp; 95<br><span class="url">example\.com\/a\?b=&quot;c&quot;<\/span><\/p>/);
  assert.doesNotMatch(html, /script|_blank/);
});

test('oldGoogle は、http(s) 以外へのリダイレクトを結果から除き、Wikipedia は中継に向ける', () => {
  const html = oldGoogle(algo(ck('javascript:alert(1)'), 'x', '') + algo('https://ja.wikipedia.org/wiki/XP', 'W', ''), '?q=xp');
  assert.doesNotMatch(html, /javascript/);
  assert.match(html, /<a href="\/api\/wikipedia\?url=https%3A%2F%2Fja\.wikipedia\.org%2Fwiki%2FXP">W<\/a>/);
});

test('oldGoogle は、検索語を書き出すときに逃がし、ページ送りを中継に向ける', () => {
  const html = oldGoogle('', '?q=%22%3Cb%3E&first=11');
  assert.match(html, /value="&quot;&lt;b>"/);
  assert.match(html, /<a href="\/api\/bing\/search\?q=%22%3Cb%3E&amp;first=1">前へ<\/a>/);
  assert.match(html, /<b style="color:#a90a08">2<\/b>/);
});

test('luckyOf は、http(s) の結果の 1 件目の行き先を返し、結果が無ければ undefined を返す', () => {
  assert.equal(luckyOf(algo(ck('javascript:alert(1)'), 'x', '') + algo('https://example.com/a', 'A', '') + algo('https://example.com/b', 'B', '')), 'https://example.com/a');
  assert.equal(luckyOf(''), undefined);
});
