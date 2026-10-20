import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import type { RateWindow } from '../server/store.ts';

// 実際の状態や statusline の入力に触れないよう、データの置き場所を一時ディレクトリに向けてから読み込む。
// 状態ファイルは、使用率の推移を持たない前の形で置いておく。
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-rate-'));
process.env.DURUN_DASHBOARD_DATA_DIR = dataDir;
fs.writeFileSync(
  path.join(dataDir, 'state.json'),
  JSON.stringify({ sessions: {}, attention: [], docs: [], outbox: [], dismissed: [], improve: { enabled: false }, errors: {}, prNotes: {}, sessionOrder: [] }),
);
const { STATUSLINE_DIR, advanceRateWindow, readRateLimits, readStatusline, recordRateLimits } = await import('../server/claude.ts');
const { onChange, state } = await import('../server/store.ts');

const FIVE_HOURS_MS = 5 * 60 * 60 * 1000;
const END = Date.UTC(2026, 9, 14, 18, 0);
const NEXT_END = END + FIVE_HOURS_MS;

test('使用率の推移を持たない前の形の状態ファイルを読むと、推移は空で始まる', () => {
  assert.deepEqual(state.rateWindows, {});
});

test('advanceRateWindow は、枠の始まりを 0% として、使用率が最後の点と変わったときだけ点を足す', () => {
  const fresh = advanceRateWindow(undefined, { usedPercent: 0, resetsAt: END }, 100);
  assert.deepEqual(fresh, { resetsAt: END, points: [] });
  const first = advanceRateWindow(fresh, { usedPercent: 3, resetsAt: END }, 200);
  assert.deepEqual(first, { resetsAt: END, points: [[200, 3]] });
  // 変わらなければ同じ値を返す。呼び出し側は、これで保存が要るかを決める。
  assert.equal(advanceRateWindow(first, { usedPercent: 3, resetsAt: END }, 300), first);
  assert.deepEqual(advanceRateWindow(first, { usedPercent: 5, resetsAt: END }, 400), { resetsAt: END, points: [[200, 3], [400, 5]] });
  assert.deepEqual(first, { resetsAt: END, points: [[200, 3]] });
});

test('advanceRateWindow は、resetsAt が変わったら前の枠の点を捨てて新しい枠から始め、リセットの時刻が無い値は記録しない', () => {
  const old: RateWindow = { resetsAt: END, points: [[200, 3], [400, 98]] };
  assert.deepEqual(advanceRateWindow(old, { usedPercent: 2, resetsAt: NEXT_END }, 500), { resetsAt: NEXT_END, points: [[500, 2]] });
  assert.deepEqual(advanceRateWindow(old, { usedPercent: 0, resetsAt: NEXT_END }, 500), { resetsAt: NEXT_END, points: [] });
  assert.equal(advanceRateWindow(old, { usedPercent: 50 }, 500), old);
  assert.equal(advanceRateWindow(old, undefined, 500), old);
  assert.equal(advanceRateWindow(undefined, { usedPercent: 50 }, 500), undefined);
});

test('recordRateLimits は statusline の入力から使用率の推移を記録して変わったときだけ配り、readRateLimits は同じ枠の推移だけを添える', () => {
  const writeStatusline = (fiveHour: { used: number; resets: number }, sevenDay: { used: number; resets: number }): void => {
    const limit = ({ used, resets }: { used: number; resets: number }) => ({ used_percentage: used, resets_at: resets });
    fs.mkdirSync(STATUSLINE_DIR, { recursive: true });
    fs.writeFileSync(path.join(STATUSLINE_DIR, 's1.json'), JSON.stringify({ rate_limits: { five_hour: limit(fiveHour), seven_day: limit(sevenDay) } }));
  };
  const percents = (key: 'fiveHour' | 'sevenDay') => readRateLimits()?.[key]?.points?.map(([, percent]) => percent);
  const resets = Math.floor(Date.now() / 1000) + 3600;
  let changes = 0;
  onChange(() => changes++);

  recordRateLimits();
  assert.deepEqual(state.rateWindows, {});
  assert.equal(changes, 0);

  writeStatusline({ used: 12, resets }, { used: 3, resets: resets + 86400 });
  const before = Date.now();
  recordRateLimits();
  const recordedAt = state.rateWindows.fiveHour?.points[0]?.[0] ?? 0;
  assert.ok(recordedAt >= before && recordedAt <= Date.now());
  assert.equal(state.rateWindows.fiveHour?.resetsAt, resets * 1000);
  assert.deepEqual([percents('fiveHour'), percents('sevenDay'), changes], [[12], [3], 1]);

  recordRateLimits();
  assert.equal(changes, 1);

  writeStatusline({ used: 15, resets }, { used: 3, resets: resets + 86400 });
  recordRateLimits();
  assert.deepEqual([percents('fiveHour'), percents('sevenDay'), changes], [[12, 15], [3], 2]);

  // リセットされて新しい枠になったら、記録し直すまでは、前の枠の推移を添えない。
  writeStatusline({ used: 1, resets: resets + 18000 }, { used: 3, resets: resets + 86400 });
  assert.equal(readRateLimits()?.fiveHour?.points, undefined);
  recordRateLimits();
  assert.deepEqual([percents('fiveHour'), percents('sevenDay'), changes], [[1], [3], 3]);
});

test('readStatusline は、更新時刻と大きさが前に読んだときと同じなら読み直さず、書かれたばかりの入力は毎回読み直す', () => {
  const write = (used: number, mtime: Date): void => {
    const file = path.join(STATUSLINE_DIR, 's2.json');
    fs.mkdirSync(STATUSLINE_DIR, { recursive: true });
    fs.writeFileSync(file, JSON.stringify({ context_window: { used_percentage: used } }));
    fs.utimesSync(file, mtime, mtime);
  };
  const old = new Date(Date.now() - 60_000);
  write(10, old);
  assert.equal(readStatusline('s2')?.usedPercent, 10);
  // 中身を変えても、更新時刻と大きさが同じなら前に読んだ値を返す。
  write(20, old);
  assert.equal(readStatusline('s2')?.usedPercent, 10);
  // 更新時刻の刻みが粗く、書き直しても時刻が変わらないファイルシステムを再現する。
  const recent = new Date();
  write(30, recent);
  assert.equal(readStatusline('s2')?.usedPercent, 30);
  write(40, recent);
  assert.equal(readStatusline('s2')?.usedPercent, 40);
});
