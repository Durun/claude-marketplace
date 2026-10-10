import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

// 実際の状態や要望に触れないよう、データの置き場所を一時ディレクトリに向けてから読み込む。
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-errors-'));
process.env.DURUN_DASHBOARD_DATA_DIR = dataDir;
const { recordError } = await import('../server/errors.ts');
const { state } = await import('../server/store.ts');

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
