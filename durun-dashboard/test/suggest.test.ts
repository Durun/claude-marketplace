import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { js, useFakeClaude } from './fakeClaude.ts';
import type { Conversation } from '../server/suggest.ts';

// 標準入力のプロンプトを書き出して呼ばれた回数を数え、発言が「失敗」なら失敗する偽の claude を置く。
// 作った概要を覚えるファイルは、実際の状態に触れないよう一時ディレクトリに置いてから読み込む。
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-suggest-'));
process.env.DURUN_DASHBOARD_DATA_DIR = path.join(tmp, 'data');
const { peekSummary, suggestReplies, suggestTitle, summarize } = await import('../server/suggest.ts');
const promptFile = path.join(tmp, 'prompt');
const callsFile = path.join(tmp, 'calls');
const output = { replies: [' 進めてください ', '', 1, '直してください'], title: ' 返答候補の表示 ', doing: ' 候補を出す ', now: ' 直し終えた ' };
useFakeClaude(
  tmp,
  `const prompt = stdin();
fs.writeFileSync(${js(promptFile)}, prompt);
fs.appendFileSync(${js(callsFile)}, '\\n');
if (/^失敗$/m.test(prompt)) {
  console.log('{"is_error":true,"result":"usage limit reached"}');
  process.exitCode = 1;
} else console.log(${js(JSON.stringify({ structured_output: output }))});`,
);
const calls = (): number => fs.readFileSync(callsFile, 'utf8').length;
const lastPrompt = (): string => fs.readFileSync(promptFile, 'utf8');

const conversation = (id: string, lastMessage: string): Conversation => ({
  id, theme: 'テーマ', name: '名前', status: 'idle', tasks: [], firstPrompt: '候補を出して', lastPrompt: '直して', lastMessage,
});

test('概要は同じやり取りには 1 回だけ作り、返答の候補は概要も材料にして作る', async () => {
  const c = conversation('s1', '直しました');
  assert.deepEqual(await summarize(c), { doing: '候補を出す', now: '直し終えた' });
  assert.match(lastPrompt(), /<status>\n入力待ち.*<first_prompt>\n候補を出して\n<\/first_prompt>\n\n<last_prompt>\n直して\n/s);

  assert.deepEqual(await suggestReplies(c), ['進めてください', '直してください']);
  assert.deepEqual(await suggestReplies(c), ['進めてください', '直してください']);
  assert.equal(calls(), 2);
  assert.match(lastPrompt(), /<summary>\nやっていること: 候補を出す\nいま: 直し終えた\n<\/summary>.*<last_message>\n直しました\n<\/last_message>$/s);
});

test('概要や候補を作れなければ理由を返し、次に頼まれたときに作り直す', async () => {
  const before = calls();
  const c = conversation('s2', '失敗');
  await assert.rejects(summarize(c), /usage limit reached/);
  // 概要が作れなくても候補は作ろうとし、その失敗を返す。
  await assert.rejects(suggestReplies(c), /usage limit reached/);
  assert.equal(calls() - before, 3);
});

test('セッション名の候補は、概要と最初の依頼も材料にして作る', async () => {
  assert.equal(await suggestTitle(conversation('s1', '直しました')), '返答候補の表示');
  assert.match(lastPrompt(), /<summary>\nやっていること: 候補を出す.*<first_prompt>\n候補を出して\n<\/first_prompt>/s);
});

test('概要は、やり取りが進んだら、前の概要に古い印を付けてすぐ返し、裏でまとめ直す', async () => {
  assert.deepEqual(await summarize(conversation('s3', '一つ目')), { doing: '候補を出す', now: '直し終えた' });
  assert.deepEqual(await summarize(conversation('s3', '二つ目')), { doing: '候補を出す', now: '直し終えた', stale: true });
});

const until = async (ok: () => boolean): Promise<void> => {
  for (let i = 0; i < 100 && !ok(); i++) await new Promise((r) => setTimeout(r, 50));
};

test('一覧の概要は、待たずに返して裏で作り、同時に作るのは 2 つまでにし、作業中は前に作った概要を返す', async () => {
  // 動いている偽の claude の数を、動いている間だけ置くファイルの数で数える。
  const runningDir = path.join(tmp, 'running');
  const countsFile = path.join(tmp, 'counts');
  fs.mkdirSync(runningDir);
  useFakeClaude(
    tmp,
    `const mine = ${js(runningDir)} + '/' + process.pid;
fs.writeFileSync(mine, '');
fs.appendFileSync(${js(countsFile)}, fs.readdirSync(${js(runningDir)}).length + '\\n');
await new Promise((resolve) => setTimeout(resolve, 300));
fs.rmSync(mine);
fs.appendFileSync(${js(callsFile)}, '\\n');
console.log(${js(JSON.stringify({ structured_output: output }))});`,
  );
  const ids = ['p1', 'p2', 'p3'];
  const peekAll = () => ids.map((id) => peekSummary(conversation(id, '進めました')));
  assert.deepEqual(peekAll(), [undefined, undefined, undefined]);
  await until(() => peekAll().every(Boolean));
  assert.deepEqual(peekSummary(conversation('p1', '進めました')), { doing: '候補を出す', now: '直し終えた' });
  const counts = fs.readFileSync(countsFile, 'utf8').trim().split('\n').map(Number);
  assert.equal(counts.length, 3);
  assert.ok(Math.max(...counts) <= 2);

  const before = calls();
  assert.deepEqual(peekSummary({ ...conversation('p1', '作業中の発言'), status: 'busy' }), { doing: '候補を出す', now: '直し終えた' });
  await new Promise((r) => setTimeout(r, 500));
  assert.equal(calls(), before);
});
