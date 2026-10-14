import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { js, useFakeClaude } from './fakeClaude.ts';

// 実際の状態に触れないよう、データの置き場所を一時ディレクトリに向けてから読み込む。
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-diagram-'));
process.env.DURUN_DASHBOARD_DATA_DIR = path.join(tmp, 'data');
const { makeDiagram } = await import('../server/diagram.ts');
const { state } = await import('../server/store.ts');

const KIT_DIR = path.join(import.meta.dirname, '..', 'server', 'diagram');

test('図解は、技法カタログを渡して本文だけを書かせ、共通の CSS を本文より前に、共通の JavaScript を本文より後に入れる', async () => {
  const promptFile = path.join(tmp, 'prompt');
  const answer = { title: ' A<B & C ', body: '<style>.mine { color: red; }</style><h1>図解</h1>' };
  useFakeClaude(tmp, `fs.writeFileSync(${js(promptFile)}, stdin());\nconsole.log(JSON.stringify({ structured_output: ${js(answer)} }));`);

  const meta = await makeDiagram('s1', tmp, [{ kind: 'prompt', text: '図にして' }, { kind: 'tool', text: 'Read' }], '');
  const prompt = fs.readFileSync(promptFile, 'utf8');
  assert.ok(prompt.includes(`<guide>\n${fs.readFileSync(path.join(KIT_DIR, 'guide.md'), 'utf8')}\n</guide>`));
  assert.ok(prompt.includes(KIT_DIR));
  assert.ok(prompt.endsWith('<chat>\n人間: 図にして\n</chat>'));

  assert.equal(meta.title, 'A<B & C');
  const html = state.docs.find((d) => d.id === meta.id)?.content ?? '';
  assert.match(html, /^<!doctype html><html lang="ja"><head><meta charset="utf-8">/);
  assert.ok(html.includes('<title>A&lt;B &amp; C</title>'));
  const at = (s: string): number => {
    const i = html.indexOf(s);
    assert.ok(i >= 0, s);
    return i;
  };
  assert.ok(at('--fg: #1a1a1a') < at('.mine { color: red; }'));
  assert.ok(at('<h1>図解</h1>') < at("outline.setAttribute('aria-label', '目次')"));
});

test('技法カタログが指す例ファイルと、置いてある例ファイルは一致する', () => {
  const named = [...fs.readFileSync(path.join(KIT_DIR, 'guide.md'), 'utf8').matchAll(/examples\/([\w-]+\.html)/g)].map((m) => m[1]);
  assert.deepEqual([...new Set(named)].sort(), fs.readdirSync(path.join(KIT_DIR, 'examples')).sort());
});
