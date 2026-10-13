import assert from 'node:assert/strict';
import { test } from 'node:test';
import { oldGoogle } from '../server/google.ts';

test('oldGoogle は、件数を数え、検索語を逃がし、Gooooooooogle のページ送りを start で中継に向ける', () => {
  const html = oldGoogle([{ href: 'https://example.com/', display: 'example.com/', title: 'E', snippet: '' }], '?q=%22%3Cb%3E&start=10');
  assert.match(html, /value="&quot;&lt;b>"/);
  assert.match(html, /<b>11<\/b> - <b>11<\/b> 件目/);
  assert.match(html, /<a href="\/api\/google\/search\?q=%22%3Cb%3E&amp;start=0"><span class="pl"><span style="color:#1851ce">G<\/span><\/span><br><span class="pn"><b>前へ<\/b><\/span><\/a>/);
  assert.match(html, /<td><span class="pl"><span style="color:#c41200">o<\/span><\/span><br><span class="pn"><b style="color:#c41200">2<\/b><\/span><\/td>/);
  assert.equal(html.split('class="pager"')[1]?.match(/>o<\/span>/g)?.length, 10);
  assert.match(html, /start=20"><span class="pl"><span style="color:#1851ce">g/);
});
