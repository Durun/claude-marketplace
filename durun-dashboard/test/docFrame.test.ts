import assert from 'node:assert/strict';
import { test } from 'node:test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { buildSrcdoc } from '../client/docFrame.ts';
import { docAsset, inlineAssets } from '../server/docAssets.ts';
import { withBase } from '../shared/html.ts';

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

test('ファイルから読んだ HTML の資料は、相対パスの起点を資料のディレクトリを返すアドレスに向け、ページ内リンクはその場でスクロールする', () => {
  const srcdoc = buildSrcdoc({ id: 'ab12', path: '/work/doc.html', format: 'html', content: '<!doctype html><html><head><title>t</title></head><body></body></html>' });
  assert.ok(srcdoc.startsWith('<!doctype html><html><head><base href="/api/docs/ab12/files/"><title>'));
  assert.match(srcdoc, /a\[href\^="#"\]/);
  // 本文で渡した資料には、相対パスの起点にするディレクトリが無い。
  assert.doesNotMatch(buildSrcdoc({ id: 'ab12', format: 'html', content: HTML }), /<base/);
});

test('withBase は、head が無ければ doctype の直後に足し、HTML が決めた base は変えない', () => {
  assert.equal(withBase('<!DOCTYPE html><p>a</p>', '/x/'), '<!DOCTYPE html><base href="/x/"><p>a</p>');
  assert.equal(withBase('<p>a</p>', '/x/'), '<base href="/x/"><p>a</p>');
  assert.equal(withBase('<head><base href="/y/"></head>', '/x/'), '<head><base href="/y/"></head>');
});

test('docAsset は、資料のディレクトリの中の音声、画像、動画だけを返す', () => {
  const doc = path.resolve('work', 'doc.html');
  assert.deepEqual(docAsset(doc, 'assets/chord.WAV'), { full: path.resolve('work', 'assets', 'chord.WAV'), type: 'audio/wav' });
  assert.equal(docAsset(doc, 'fig.png')?.type, 'image/png');
  assert.equal(docAsset(doc, '../secret.png'), undefined);
  assert.equal(docAsset(doc, path.resolve('other', 'a.mp3')), undefined);
  assert.equal(docAsset(doc, 'notes.md'), undefined);
});

test('inlineAssets は、相対パスで埋め込んだ音声を data URL にして HTML の中に入れ、外のアドレスと無いファイルはそのままにする', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'doc-assets-'));
  fs.mkdirSync(path.join(dir, 'assets'));
  fs.writeFileSync(path.join(dir, 'assets', 'a b.ogg'), 'ogg');
  const html = '<audio src="assets/a%20b.ogg?v=1"></audio><img src="https://example.com/x.png"><video src=\'gone.mp4\' poster="../out.png"></video>';
  const data = `data:audio/ogg;base64,${Buffer.from('ogg').toString('base64')}`;
  assert.equal(inlineAssets(html, path.join(dir, 'doc.html')), html.replace('assets/a%20b.ogg?v=1', data));
  fs.rmSync(dir, { recursive: true });
});
