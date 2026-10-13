import assert from 'node:assert/strict';
import { test } from 'node:test';
import { rewriteBing } from '../server/bing.ts';

const SELF = 'http://localhost:7337';
const ck = (to: string) => `/ck/a?!&amp;&amp;p=abc&amp;u=a1${Buffer.from(to).toString('base64url')}&amp;ntb=1`;

test('rewriteBing は、スクリプトと target="_blank" を除き、結果のリンクを行き先そのものにする', () => {
  const html = `<html><head><title>t</title><script>alert(1)</script></head><body><a href="https://www.bing.com${ck('https://ja.wikipedia.org/?a=1&b="2"')}" target="_blank">W</a></body></html>`;
  assert.equal(rewriteBing(html, SELF), '<html><head><base href="https://www.bing.com/"><title>t</title></head><body><a href="https://ja.wikipedia.org/?a=1&amp;b=%222%22">W</a></body></html>');
});

test('rewriteBing は、http(s) 以外へのリダイレクトを書き換えない', () => {
  const link = `<a href="${ck('javascript:alert(1)')}">x</a>`;
  assert.equal(rewriteBing(link, SELF), link);
});

test('rewriteBing は、検索欄と次のページを、ダッシュボードの中継に向ける', () => {
  const html = '<form action="/search" id="sb_form"></form><a href="/search?q=a&amp;first=11">次へ</a><a href="/images/search?q=a">画像</a>';
  assert.equal(rewriteBing(html, SELF), '<form action="http://localhost:7337/api/bing/search" id="sb_form"></form><a href="http://localhost:7337/api/bing/search?q=a&amp;first=11">次へ</a><a href="/images/search?q=a">画像</a>');
});
