import assert from 'node:assert/strict';
import { test } from 'node:test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { buildSrcdoc, linkPath } from '../client/docFrame.ts';
import { docAsset, inlineAssets } from '../server/docAssets.ts';
import { withBase } from '../shared/html.ts';

const HTML = '<!doctype html><html><body><p>本文</p></body></html>';

test('XP のスキンでは、資料の iframe にスクロールバーの見た目を入れる', () => {
  assert.match(buildSrcdoc({ format: 'markdown', content: '# 見出し' }, 'xp'), /<style>\s*::-webkit-scrollbar \{.*<\/style><\/head>/s);
  assert.match(buildSrcdoc({ format: 'svg', content: '<svg></svg>' }, 'xp'), /^<svg><\/svg><style>\s*::-webkit-scrollbar /);
});

test('XP のスキンでは、markdown の資料を MS ゴシックの文字と昔の IE の表で描き、デフォルトでは描かない', () => {
  const MS_GOTHIC = /"MS PGothic".*border-spacing: 2px/s;
  assert.match(buildSrcdoc({ format: 'markdown', content: '# 見出し' }, 'xp'), MS_GOTHIC);
  assert.doesNotMatch(buildSrcdoc({ format: 'markdown', content: '# 見出し' }), MS_GOTHIC);
  assert.doesNotMatch(buildSrcdoc({ format: 'html', content: HTML }, 'xp'), MS_GOTHIC);
});

test('HTML の資料は doctype を先頭に残し、ページ内リンクの扱いとスクロールバーの見た目を末尾に足す', () => {
  const srcdoc = buildSrcdoc({ format: 'html', content: HTML }, 'xp');
  assert.ok(srcdoc.startsWith(HTML));
  assert.match(srcdoc.slice(HTML.length), /^<script>.*a\[href\^="#"\].*<\/script><style>\s*::-webkit-scrollbar /s);
});

test('デフォルトのスキンでは、資料にスクロールバーの見た目を足さない', () => {
  assert.doesNotMatch(buildSrcdoc({ format: 'html', content: HTML }), /::-webkit-scrollbar/);
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

test('linkPath は、相対パスのリンクを資料のファイルがあるディレクトリから辿り、絶対パスはそのまま返す', () => {
  assert.equal(linkPath('/home/me/memory/MEMORY.md', 'feedback.md'), '/home/me/memory/feedback.md');
  // marked が URL の形に直した文字は戻し、ページ内の場所は外す。% だけの名前は戻せないので、そのまま使う。
  assert.equal(linkPath('/home/me/memory/MEMORY.md', 'sub/%E3%83%A1%E3%83%A2%20a.md#%E8%A6%8B'), '/home/me/memory/sub/メモ a.md');
  assert.equal(linkPath('/home/me/memory/MEMORY.md', '100%.md'), '/home/me/memory/100%.md');
  assert.equal(linkPath('/home/me/a.md', '/work/b.md'), '/work/b.md');
  assert.equal(linkPath('/home/me/a.md', '~/notes/b.md'), '~/notes/b.md');
  assert.equal(linkPath('C:/notes/a.md', 'C:/work/b.md'), 'C:/work/b.md');
  assert.equal(linkPath('C:\\notes\\a.md', 'b.md'), 'C:\\notes\\b.md');
  // セッションの作業ディレクトリからの相対パスで開いたファイルは、辿った先も作業ディレクトリからの相対パスにする。
  assert.equal(linkPath('docs/guide.md', '../README.md'), 'docs/../README.md');
  assert.equal(linkPath('README.md', 'docs/guide.md'), 'docs/guide.md');
});
