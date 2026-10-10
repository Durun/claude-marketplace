import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { js, useFakeClaude } from './fakeClaude.ts';

// 実際の状態や要望に触れないよう、データの置き場所を一時ディレクトリに向けてから読み込む。
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-improve-'));
process.env.DURUN_DASHBOARD_DATA_DIR = dataDir;
const { ensureImprover, improveState, nextImprovement, saveFeedback, updateImprove } = await import('../server/improve.ts');
const { ensureSession, onChange, state } = await import('../server/store.ts');

const feedbackDir = path.join(dataDir, 'feedback');
const pending = (): string[] => fs.readdirSync(feedbackDir).filter((f) => f.endsWith('.json')).sort();
const done = (): string[] => fs.readdirSync(path.join(feedbackDir, 'done')).sort();

/** 自分のプロセスを、最後に text と発言した改善のセッションに見立てる。生きているので、新しくは起動しない。 */
const fakeRun = (sessionId: string, text: string): void => {
  const transcript = path.join(dataDir, `${sessionId}.jsonl`);
  fs.writeFileSync(transcript, `${JSON.stringify({ type: 'assistant', message: { content: [{ type: 'text', text }] } })}\n`);
  ensureSession(sessionId, dataDir).transcriptPath = transcript;
  state.improve.run = { sessionId, pid: process.pid, items: pending(), startedAt: 0 };
};

test('ウェーブを終えたら、最後の発言で対応したとされた要望だけを済みにし、残りは次のウェーブに回す', async () => {
  for (const text of ['一つ目', '二つ目', '三つ目']) {
    saveFeedback(text, [], {});
    await new Promise((r) => setTimeout(r, 5));
  }
  const [first = '', second = '', third = ''] = pending();
  // Stop の時点では、最後の発言が会話ログにまだ書かれていないことがある。
  fakeRun('s1', '作業しています。');
  // ウェーブに渡した要望は、残りの数に入れない。
  assert.deepEqual([improveState().running, improveState().pending], [true, 0]);

  nextImprovement('other');
  assert.ok(state.improve.run);
  nextImprovement('s1', `まとめて直しました。\n${second} は次に回します。\n対応した要望:\n${first}\n${third}`);
  assert.equal(state.improve.run, undefined);
  assert.deepEqual(pending(), [second]);
  assert.deepEqual(done(), [first, third]);
  assert.match(JSON.parse(fs.readFileSync(path.join(feedbackDir, 'done', first), 'utf8')).result, /まとめて直しました/);
});

test('対応した要望を書かずに止まったウェーブでは、同じ計画を繰り返さないよう、ON のまま待機する', () => {
  state.improve.enabled = true;
  fakeRun('s2', '直しました。');
  nextImprovement('s2');
  assert.equal(state.improve.enabled, true);
  assert.equal(state.improve.run, undefined);
  assert.match(state.improve.error ?? '', /対応した要望/);
  assert.equal(pending().length, 1);
});

test('待機中に要望が届くと、たまっている要望をすべて渡して再開し、失敗したら ON のまま理由を残して、続けるまで待つ', async () => {
  // 偽の claude は、受け取った引数を書き出してすぐ失敗する。
  const bin = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-bin-'));
  const argsFile = path.join(bin, 'args');
  useFakeClaude(bin, `fs.writeFileSync(${js(argsFile)}, lines(args)); console.log('fake failure'); process.exitCode = 1;`);
  const failed = () => new Promise<void>((resolve) => onChange(() => state.improve.error?.includes('fake') && resolve()));
  let stopped = failed();
  saveFeedback('四つ目', [], {});
  await stopped;
  const [second = '', fourth = ''] = pending();
  const args = fs.readFileSync(argsFile, 'utf8');
  assert.match(args, new RegExp(`2 件たまっています[\\s\\S]*${second}: 二つ目[\\s\\S]*${fourth}: 四つ目`));
  // 人間の確認を待たずに進めるよう、コードレビューの依頼は使わせない。
  assert.match(args, /^--disallowedTools\nmcp__plugin_durun-dashboard_durun-dashboard__request_review$/m);
  assert.equal(state.improve.enabled, true);
  assert.match(state.improve.error ?? '', /終了コード 1.*fake failure/);
  assert.equal(improveState().pending, 2);

  fs.rmSync(argsFile);
  ensureImprover();
  assert.equal(fs.existsSync(argsFile), false);
  stopped = failed();
  updateImprove({ enabled: true });
  await stopped;
  assert.equal(fs.existsSync(argsFile), true);
});
