import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

// 実際の状態や要望に触れないよう、データの置き場所を一時ディレクトリに向けてから読み込む。
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-errors-'));
process.env.DURUN_DASHBOARD_DATA_DIR = dataDir;
const { recordError, recordSlow } = await import('../server/errors.ts');
const { nextImprovement } = await import('../server/improve.ts');
const { ensureSession, state } = await import('../server/store.ts');
const { platform } = await import('../server/platform.ts');

const feedbackDir = path.join(dataDir, 'feedback');
const pending = (): string[] => fs.readdirSync(feedbackDir).filter((f) => f.endsWith('.json')).sort();
const finish = (item: string): void => {
  fs.mkdirSync(path.join(feedbackDir, 'done'), { recursive: true });
  fs.renameSync(path.join(feedbackDir, item), path.join(feedbackDir, 'done', item));
};
const stack = (line: number): string => `Error: x\n    at buildSession (file:///repo/server/snapshot.ts:${line}:12)`;

test('同じエラーは 1 件の要望にまとめ、未対応のエラーの要望は 3 件までにし、対応の後に再発したら 1 回だけ起票し直す', () => {
  recordError('server', 'unknown session: 3c10df45-64fa-4b96-b788-eed91476a530', stack(97), 'GET /api/x');
  recordError('server', 'unknown session: abcdef01-0000-abcd-ef00-000000000000', stack(99), 'GET /api/x');
  const [first = ''] = pending();
  assert.equal(pending().length, 1);
  assert.equal(Object.values(state.errors)[0]?.count, 2);
  assert.match(JSON.parse(fs.readFileSync(path.join(feedbackDir, first), 'utf8')).text, /サーバー（GET \/api\/x）.*unknown session/s);

  for (const name of ['a', 'b', 'c']) recordError('client', `TypeError: ${name} is undefined`);
  assert.equal(pending().length, 3);

  finish(first);
  recordError('server', 'unknown session: 1', stack(97));
  const refiled = pending().find((f) => !Object.values(state.errors).some((e) => e.source === 'client' && e.item === f)) ?? '';
  assert.match(JSON.parse(fs.readFileSync(path.join(feedbackDir, refiled), 'utf8')).text, new RegExp(`前回の要望（${first}）`));

  finish(refiled);
  recordError('server', 'unknown session: 2', stack(97));
  assert.equal(pending().length, 2);
});

test('遅い処理は、同じ処理を 1 件の要望にまとめて最長の時間を数え、未対応の要望はエラーとは別に 3 件までにする', () => {
  const before = pending();
  recordSlow('server', 'GET ^/api/x$ の応答（見込み 1 秒）', 1500, 'GET /api/x');
  recordSlow('server', 'GET ^/api/x$ の応答（見込み 1 秒）', 3200, 'GET /api/x');
  const [slow = ''] = pending().filter((f) => !before.includes(f));
  const text = JSON.parse(fs.readFileSync(path.join(feedbackDir, slow), 'utf8')).text;
  assert.match(text, /見込みより時間のかかった処理.*サーバー（GET \/api\/x）.*遅い処理: GET \^\/api\/x\$ の応答.*1 回（最長 1\.5 秒/s);
  // 負荷を測れる OS では、最長だった回の load average を添える。
  if (platform.loadAverage() !== undefined) assert.match(text, /最長だったときのサーバーの負荷: load average \d+\.\d（直近 1 分の平均、CPU \d+ 個）/);
  assert.deepEqual(Object.values(state.errors).filter((e) => e.item === slow).map((e) => [e.count, e.slowestMs]), [[2, 3200]]);

  for (const name of ['a', 'b', 'c']) recordSlow('client', `home の画面を固めた「${name}」の処理`, 600);
  assert.equal(pending().length - before.length, 3);
  recordError('client', 'TypeError: d is undefined');
  assert.equal(pending().length - before.length, 4);
});

test('要望に対応した後に再発したら、回数と最長をその後の回から数え直して起票し直す', () => {
  for (const item of pending()) finish(item);
  const message = 'session/overview の画面を固めた「FrameRequestCallback」の処理（見込み 0.5 秒）';
  recordSlow('client', message, 10453);
  const [item = ''] = pending();
  // 改善のセッションが、この要望に対応したと書いて止まる。
  const transcript = path.join(dataDir, 'wave.jsonl');
  fs.writeFileSync(transcript, `${JSON.stringify({ type: 'assistant', message: { content: [{ type: 'text', text: '直しました。' }] } })}\n`);
  ensureSession('wave', dataDir).transcriptPath = transcript;
  state.improve.run = { sessionId: 'wave', pid: process.pid, items: [item], startedAt: 0 };
  nextImprovement('wave', `直しました。\n対応した要望:\n${item}`);

  const since = Date.now();
  recordSlow('client', message, 800);
  const [refiled = ''] = pending();
  assert.match(JSON.parse(fs.readFileSync(path.join(feedbackDir, refiled), 'utf8')).text, new RegExp(`1 回（最長 0\\.8 秒.*前回の要望（${item}）`, 's'));
  assert.ok((Object.values(state.errors).find((e) => e.item === refiled)?.firstAt ?? 0) >= since);
});
