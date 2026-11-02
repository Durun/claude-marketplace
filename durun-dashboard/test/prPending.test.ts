import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { tempDir } from './tmp.ts';

// 読んだ PR を覚えるファイルは、実際の状態に触れないよう一時ディレクトリに置いてから読み込む。
// 古くなったバッジの PR を、マージ済みと開いているものの 2 つ覚えさせておく。
const dataDir = tempDir('durun-dashboard-pr-');
process.env.DURUN_DASHBOARD_DATA_DIR = dataDir;
const pr = (n: number, state: string) => ({ value: { url: `https://github.com/o/r/pull/${n}`, number: n, title: 't', state }, at: Date.now() - 60 * 60_000 });
fs.writeFileSync(path.join(dataDir, 'cache.json'), JSON.stringify({ 'pr-badge:https://github.com/o/r/pull/1': pr(1, 'MERGED'), 'pr-badge:https://github.com/o/r/pull/2': pr(2, 'OPEN') }));
const { badgePr, prPending, refreshPr } = await import('../server/integrations.ts');
const { refreshing } = await import('../server/cache.ts');

test('prPending は、ブランチの PR を読み終えるまで真で、PR が無いと分かった後と、main のように読まないブランチでは偽になる', async () => {
  // git のリポジトリではないので、gh は PR を読めず、PR が無いと覚える。
  const cwd = tempDir('durun-dashboard-pr-cwd-');
  assert.equal(prPending(cwd, 'feature/x'), true);
  await refreshPr(cwd, 'feature/x');
  assert.equal(prPending(cwd, 'feature/x'), false);
  assert.equal(prPending(cwd, 'main'), false);
});

test('badgePr は、古くなった PR を裏で読み直すが、マージ済みの PR と、keep を渡した PR は読み直さない', (t) => {
  t.mock.method(console, 'error', () => {});
  assert.equal(badgePr('https://github.com/o/r/pull/1')?.state, 'MERGED');
  assert.equal(refreshing('pr-badge:https://github.com/o/r/pull/1'), false);
  assert.equal(badgePr('https://github.com/o/r/pull/2', true)?.state, 'OPEN');
  assert.equal(refreshing('pr-badge:https://github.com/o/r/pull/2'), false);
  assert.equal(badgePr('https://github.com/o/r/pull/2')?.state, 'OPEN');
  assert.equal(refreshing('pr-badge:https://github.com/o/r/pull/2'), true);
});
