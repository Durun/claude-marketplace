import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderRetro } from '../server/retro.ts';
import { frameOf } from '../shared/retro.ts';

const ARTICLE = new URL('https://www.jiji.com/jc/article?k=1');
const long = (s: string) => s.repeat(20);

test('frameOf は、昔の見た目で描けるサイトだけを中継に向ける', () => {
  assert.equal(frameOf('https://www.bing.com/search?q=a'), '/api/bing/search?q=a');
  assert.equal(frameOf('https://ja.wikipedia.org/wiki/A'), '/api/wikipedia?url=https%3A%2F%2Fja.wikipedia.org%2Fwiki%2FA');
  assert.equal(frameOf('https://www.jiji.com/jc/a'), '/api/retro?url=https%3A%2F%2Fwww.jiji.com%2Fjc%2Fa');
  assert.equal(frameOf('http://www.jiji.com/'), 'http://www.jiji.com/');
  assert.equal(frameOf('https://example.com/'), 'https://example.com/');
});

test('renderRetro は、記事のページの長い段落を本文にし、メニューの中の段落と短い段落を除く', () => {
  const html = renderRetro(`<meta property="og:title" content="a < b"><nav><p>${long('メニュー')}</p></nav><p>短い</p><p><b>${long('本文')}</b></p><script>x()</script>`, ARTICLE);
  assert.match(html, /<h1>a &lt; b<\/h1>/);
  assert.match(html, new RegExp(`<p>${long('本文')}</p>`));
  assert.doesNotMatch(html, /メニュー|短い|script/);
});

test('renderRetro は、見出しらしい長さのリンクを重複を除いて集め、描けるサイトへのリンクは中継に向ける', () => {
  const html = renderRetro('<a href="/jc/b">ふたつめの記事の見出し</a><a href="/jc/b">ふたつめの記事の見出し</a><a href="https://example.com/x">よそのサイトの記事の見出し</a><a href="/top">トップ</a><a href="javascript:alert(1)">スクリプトのリンクの見出し</a>', ARTICLE);
  assert.equal(html.match(/<li>/g)?.length, 2);
  assert.match(html, /<li><a href="\/api\/retro\?url=https%3A%2F%2Fwww\.jiji\.com%2Fjc%2Fb">ふたつめの記事の見出し<\/a><\/li>/);
  assert.match(html, /<li><a href="https:\/\/example\.com\/x">よそのサイトの記事の見出し<\/a><\/li>/);
  assert.doesNotMatch(html, /javascript/);
});

test('renderRetro は、トップページと記事の一覧だけのサイトでは、段落を本文として出さない', () => {
  const p = `<p>${long('要約')}</p>`;
  assert.doesNotMatch(renderRetro(p, new URL('https://www.jiji.com/')), /要約/);
  assert.doesNotMatch(renderRetro(p, new URL('https://b.hatena.ne.jp/hotentry/all')), /要約/);
});
