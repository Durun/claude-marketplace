import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { js, useFakeClaude } from './fakeClaude.ts';
import type { Conversation } from '../server/suggest.ts';
import type { ChatEntry } from '../shared/types.ts';

// 標準入力のプロンプトを書き出して呼ばれた回数を数え、発言が「失敗」なら失敗する偽の claude を置く。
// 作った概要を覚えるファイルは、実際の状態に触れないよう一時ディレクトリに置いてから読み込む。
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-suggest-'));
process.env.DURUN_DASHBOARD_DATA_DIR = path.join(tmp, 'data');
const { explainTerm, peekSummary, sessionGlossary, suggestReplies, suggestTitle, summarize } = await import('../server/suggest.ts');
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

// 用語集を作る偽の claude。やり取りの中の「TERM:語」を拾い、FAIL があれば失敗する。説明を頼まれたら、全角と半角の違いを除いて、材料に語が出てくるかを説明に書く。
const useGlossaryClaude = (): void =>
  useFakeClaude(
    tmp,
    `const prompt = stdin();
fs.appendFileSync(${js(callsFile)}, '\\n');
const term = /<term>\\n(.*)\\n<\\/term>/.exec(prompt)?.[1];
const around = /<conversation>\\n([\\s\\S]*)\\n<\\/conversation>/.exec(prompt)?.[1] ?? '';
if (prompt.includes('FAIL')) {
  console.log('{"is_error":true,"result":"usage limit reached"}');
  process.exitCode = 1;
} else if (term) console.log(JSON.stringify({ structured_output: { description: term + (around.normalize('NFKC').includes(term) ? ' が出てくる' : ' は出てこない') } }));
else console.log(JSON.stringify({ structured_output: { terms: [...prompt.matchAll(/TERM:(\\w+)/g)].map((m) => ({ term: m[1], description: m[1] + ' の説明' })) } }));`,
  );

const prompt = (text: string): ChatEntry => ({ kind: 'prompt', text });
const message = (text: string): ChatEntry => ({ kind: 'message', text });
// 材料の 1 区切り（4000 字）より長くして、次の発言を次の区切りに入れる。
const FILLER = 'あ'.repeat(4100);

test('用語集は、やり取りを区切って拾った語を新しい順に、人間が足した語より後に並べ、同じ語と外した語は除き、同じやり取りは拾い直さない', async () => {
  useGlossaryClaude();
  const before = calls();
  const entries = [prompt('TERM:alpha を調べて'), { kind: 'tool', text: 'Bash TERM:tool' } as ChatEntry, message(`TERM:beta です${FILLER}`), message('TERM:gamma と TERM:Alpha')];
  const saved = [{ term: '古い語', description: '先に足した', at: 1 }, { term: '新しい語', description: '後で足した', at: 2 }];
  const expected = {
    terms: [
      { term: '新しい語', description: '後で足した' },
      { term: '古い語', description: '先に足した' },
      { term: 'Alpha', description: 'Alpha の説明' },
      { term: 'gamma', description: 'gamma の説明' },
    ],
  };
  assert.deepEqual(await sessionGlossary('g1', entries, false, saved, ['beta']), expected);
  assert.equal(calls() - before, 2);
  assert.deepEqual(await sessionGlossary('g1', entries, false, saved, ['beta']), expected);
  assert.equal(calls() - before, 2);
});

test('用語集は、作業中は伸びた最後の区切りを拾い直さず、止まったら拾い直す。拾えなかった区切りがあれば、拾えた語に理由を添える', async () => {
  useGlossaryClaude();
  const termsOf = async (entries: ChatEntry[], busy: boolean): Promise<string[]> => (await sessionGlossary('g2', entries, busy, [], [])).terms.map((t) => t.term);
  const entries = [message(`TERM:alpha${FILLER}`), message('TERM:beta')];
  await termsOf(entries, false);
  const before = calls();
  const longer = [...entries, message('TERM:gamma')];
  assert.deepEqual(await termsOf(longer, true), ['beta', 'alpha']);
  assert.equal(calls(), before);
  // 止まっていれば、前に拾った語をすぐ返し、裏で拾い直す。
  let terms = await termsOf(longer, false);
  assert.deepEqual(terms, ['beta', 'alpha']);
  for (let i = 0; i < 100 && !terms.includes('gamma'); i++) {
    await new Promise((r) => setTimeout(r, 50));
    terms = await termsOf(longer, false);
  }
  assert.deepEqual(terms, ['gamma', 'beta', 'alpha']);
  assert.equal(calls(), before + 1);

  const failing = [message(`TERM:delta${FILLER}`), message('FAIL')];
  assert.deepEqual(await sessionGlossary('g3', failing, false, [], []), { terms: [{ term: 'delta', description: 'delta の説明' }], error: 'usage limit reached' });
});

test('説明を書かずに足した語は、語が出てくるあたりのやり取りを材料に説明を書かせる', async () => {
  useGlossaryClaude();
  const entries = [prompt(`${FILLER}${FILLER}`), message('ここで ＧＫＥ を使います'), message(FILLER)];
  assert.equal(await explainTerm('GKE', entries), 'GKE が出てくる');
  assert.equal(await explainTerm('dind', entries), 'dind は出てこない');
});
