import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import type { PrInfo } from '../shared/types.ts';

// 読んだ PR を覚えるファイルは、実際の状態に触れないよう一時ディレクトリに置いてから読み込む。
const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-claude-pr-'));
process.env.DURUN_DASHBOARD_DATA_DIR = dir;
const { cached } = await import('../server/cache.ts');
const { badgePr, mergeClaudePrCache } = await import('../server/integrations.ts');

const url = 'https://github.com/o/r/pull/1';
const checks = { passed: 1, failed: 0, pending: 0 };
const pr: PrInfo = { url, number: 1, title: 't', state: 'OPEN', isDraft: false, review: '', checks };

/** バッジの PR として、gh で読んだ値を今の時刻に覚えさせる。 */
const ghRead = (value: PrInfo) => cached(`pr-badge:${url}`, { fresh: true }, async () => value);

/** Claude Code が PR の状態をファイルに書いたことにする。at は書いた時刻。 */
const claudeWrite = (state: string, at: number): void => {
  const file = path.join(dir, 'gh-pr-status-cache.json');
  fs.writeFileSync(file, JSON.stringify({ [url]: { number: 1, title: 't', state, checks, review: null, additions: 1, deletions: 0 } }));
  fs.utimesSync(file, at / 1000, at / 1000);
  mergeClaudePrCache(file);
};

test('Claude Code が書いた PR の状態は、gh で読んだ後に変わったときだけ重ね、DRAFT は下書きのオープンとして出す', async (t) => {
  t.mock.timers.enable({ apis: ['Date'], now: 1_000_000 });
  await ghRead(pr);
  claudeWrite('DRAFT', 1_001_000);
  assert.equal(badgePr(url)?.state, 'OPEN');
  assert.equal(badgePr(url)?.isDraft, true);

  t.mock.timers.tick(2000);
  await ghRead({ ...pr, state: 'MERGED' });
  assert.equal(badgePr(url)?.state, 'MERGED');

  // ほかの PR の状態が変わって書き直されても、変わっていない古い状態は重ねない。
  claudeWrite('DRAFT', 1_003_000);
  assert.equal(badgePr(url)?.state, 'MERGED');
});
