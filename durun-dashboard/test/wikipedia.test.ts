import assert from 'node:assert/strict';
import { test } from 'node:test';
import { rewriteWikipedia } from '../server/wikipedia.ts';
import { isWikipedia } from '../shared/wikipedia.ts';

const SELF = 'http://localhost:7337';
const PAGE = new URL('https://ja.wikipedia.org/wiki/XP');
const HERE = `${SELF}/api/wikipedia?url=https%3A%2F%2Fja.wikipedia.org%2Fwiki%2FXP`;

test('rewriteWikipedia は、スクリプトを除き、各言語版の記事へのリンクを中継に向け、ページ内のリンクをこのページに付け直す', () => {
  const html = '<html><head><script>x()</script></head><body><a href="/wiki/A?x=1&amp;y=2">A</a><a href="//en.wikipedia.org/wiki/B">B</a><a href="https://ja.wikipedia.org/wiki/C#s">C</a><a href="#top">T</a><a href="/w/index.php?title=A">E</a></body></html>';
  assert.equal(
    rewriteWikipedia(html, PAGE, HERE, SELF),
    `<html><head><base href="https://ja.wikipedia.org/wiki/XP"></head><body><a href="${SELF}/api/wikipedia?url=https%3A%2F%2Fja.wikipedia.org%2Fwiki%2FA%3Fx%3D1%26y%3D2">A</a><a href="${SELF}/api/wikipedia?url=https%3A%2F%2Fen.wikipedia.org%2Fwiki%2FB">B</a><a href="${SELF}/api/wikipedia?url=https%3A%2F%2Fja.wikipedia.org%2Fwiki%2FC%23s">C</a><a href="${HERE}#top">T</a><a href="/w/index.php?title=A">E</a></body></html>`,
  );
});

test('isWikipedia は、https の各言語版の Wikipedia だけを中継してよいとする', () => {
  assert.ok(isWikipedia(new URL('https://ja.wikipedia.org/wiki/XP')));
  for (const u of ['http://ja.wikipedia.org/', 'https://wikipedia.org.evil.example/', 'https://evil.example/?ja.wikipedia.org', 'https://a.b.wikipedia.org/']) assert.ok(!isWikipedia(new URL(u)), u);
});
