import assert from 'node:assert/strict';
import { test } from 'node:test';
import { buildSrcdoc } from '../client/docFrame.ts';

const HTML = '<!doctype html><html><body><p>本文</p></body></html>';

test('XP のスキンでは、資料の iframe にスクロールバーの見た目を入れる', () => {
  assert.match(buildSrcdoc({ format: 'markdown', content: '# 見出し' }, 'xp'), /<style>\s*::-webkit-scrollbar \{.*<\/style><\/head>/s);
  assert.match(buildSrcdoc({ format: 'svg', content: '<svg></svg>' }, 'xp'), /^<svg><\/svg><style>\s*::-webkit-scrollbar /);
});

test('HTML の資料は doctype を先頭に残し、スクロールバーの見た目を末尾に足す', () => {
  const srcdoc = buildSrcdoc({ format: 'html', content: HTML }, 'xp');
  assert.ok(srcdoc.startsWith(HTML));
  assert.match(srcdoc.slice(HTML.length), /^<style>\s*::-webkit-scrollbar /);
});

test('デフォルトのスキンでは、資料にスクロールバーの見た目を足さない', () => {
  assert.equal(buildSrcdoc({ format: 'html', content: HTML }), HTML);
  assert.doesNotMatch(buildSrcdoc({ format: 'markdown', content: '# 見出し' }), /::-webkit-scrollbar/);
});
