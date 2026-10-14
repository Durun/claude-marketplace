import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { js, useFakeClaude } from './fakeClaude.ts';
import { piecesOf, sentencesOf } from '../client/proofread.ts';
import { readSentence } from '../server/proofread.ts';

test('文章は文ごとに切り、空白は前の文につなげ、コードブロックは丸ごと 1 つにする', () => {
  const text = '\n## 方針\n\n案Bで進める。これで解消する！\n```ts\nconst a = "x。y";\n```\n- 項目\n';
  const sentences = sentencesOf(text);
  assert.deepEqual(sentences, ['## 方針\n\n', '案Bで進める。', 'これで解消する！\n', '```ts\nconst a = "x。y";\n```\n', '- 項目']);
  assert.equal(sentences.join(''), text.trim());
});

test('引っかかった語句は、文の中で最初に現れた所に印を付け、見つからない語句と重なる語句には付けない', () => {
  const q = (quote: string) => ({ quote, question: `${quote}って？` });
  assert.deepEqual(piecesOf('これにより、例の問題は解消する。', [q('例の問題'), q('これ'), q('例の'), q('案B')]), [
    { text: 'これ', question: 'これって？' },
    { text: 'により、' },
    { text: '例の問題', question: '例の問題って？' },
    { text: 'は解消する。' },
  ]);
});

test('読み手には、ここまで読んだ部分と今の文だけを渡し、答えの形が崩れていれば失敗にする', async () => {
  // 標準入力のプロンプトを書き出し、今の文が「壊れた答え」なら判定の欠けた答えを返す偽の claude を置く。
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-proofread-'));
  const promptFile = path.join(tmp, 'prompt');
  const verdict = { level: 'partly', note: ' 指す先が分からない ' };
  const reading = { understand: verdict, role: { level: 'yes', note: '結論' }, foresee: { level: 'no', note: '見当がつかない' }, questions: [{ quote: ' 例の問題 ', question: ' 何のこと？ ' }, { quote: '', question: '空' }] };
  useFakeClaude(
    tmp,
    `const prompt = stdin();
fs.writeFileSync(${js(promptFile)}, prompt);
const broken = prompt.includes('壊れた答え');
console.log(JSON.stringify({ structured_output: broken ? { understand: { level: 'maybe', note: '' } } : ${js(reading)} }));`,
  );

  assert.deepEqual(await readSentence('## 方針\n\n案Bで進める。', 'これにより、例の問題は解消する。\n'), {
    understand: { level: 'partly', note: '指す先が分からない' },
    role: { level: 'yes', note: '結論' },
    foresee: { level: 'no', note: '見当がつかない' },
    questions: [{ quote: '例の問題', question: '何のこと？' }],
  });
  assert.match(fs.readFileSync(promptFile, 'utf8'), /<read>\n## 方針\n\n案Bで進める。\n<\/read>\n\n<sentence>\nこれにより、例の問題は解消する。\n<\/sentence>$/);

  // 最初の文では、まだ何も読んでいないと伝える。
  await readSentence('', '## 方針');
  assert.match(fs.readFileSync(promptFile, 'utf8'), /<read>\n（まだ何も読んでいません）\n<\/read>/);

  await assert.rejects(readSentence('', '壊れた答え'), /読み手の答えが返りませんでした/);
});
