import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

// 読んだ PR を覚えるファイルは、実際の状態に触れないよう一時ディレクトリに置いてから読み込む。
process.env.DURUN_DASHBOARD_DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-pr-'));
const { prPending, refreshPr } = await import('../server/integrations.ts');

test('prPending は、ブランチの PR を読み終えるまで真で、PR が無いと分かった後と、main のように読まないブランチでは偽になる', async () => {
  // git のリポジトリではないので、gh は PR を読めず、PR が無いと覚える。
  const cwd = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-pr-cwd-'));
  assert.equal(prPending(cwd, 'feature/x'), true);
  await refreshPr(cwd, 'feature/x');
  assert.equal(prPending(cwd, 'feature/x'), false);
  assert.equal(prPending(cwd, 'main'), false);
});
