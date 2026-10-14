import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { RatePoint } from '../shared/types.ts';
import { FIVE_HOURS_MS, SEVEN_DAYS_MS, paceOf, rateGraphShape } from '../client/rateGraph.ts';

// 四角は x が 6〜234、y が 6（100%）〜50（0%）。
const END = Date.UTC(2026, 9, 14, 18, 0);
const at = (ratio: number): number => END - FIVE_HOURS_MS + FIVE_HOURS_MS * ratio;

test('rateGraphShape は、点が無くても、枠の始まりの 0% から今の使用率までを直線で結び、今の位置と均等なペースを返す', () => {
  assert.deepEqual(rateGraphShape({ usedPercent: 10, resetsAt: END }, FIVE_HOURS_MS, at(0.5)), {
    path: 'M6,50L120,45.6',
    dot: { x: 120, y: 45.6 },
    pace: 50,
    pacePath: 'M6,50L234,6',
    off: [],
  });
  assert.equal(rateGraphShape({ usedPercent: 0, resetsAt: END }, FIVE_HOURS_MS, at(0))?.path, 'M6,50L6,50');
});

test('rateGraphShape は、最初の点までを 0% から直線で結び、その先は記録した点で折れる階段を、今の時刻まで同じ使用率で伸ばす', () => {
  const points: RatePoint[] = [[at(0.25), 10], [at(0.5), 30]];
  assert.deepEqual(rateGraphShape({ usedPercent: 30, resetsAt: END, points }, FIVE_HOURS_MS, at(0.75)), {
    path: 'M6,50L63,45.6H120V36.8H177V36.8',
    dot: { x: 177, y: 36.8 },
    pace: 75,
    pacePath: 'M6,50L234,6',
    off: [],
  });
});

test('rateGraphShape は、枠の長さに合わせて横軸を割り当て、リセットを過ぎた今は右端と 100% で止める', () => {
  assert.deepEqual(rateGraphShape({ usedPercent: 50, resetsAt: END }, SEVEN_DAYS_MS, END - SEVEN_DAYS_MS / 2), {
    path: 'M6,50L120,28',
    dot: { x: 120, y: 28 },
    pace: 50,
    pacePath: 'M6,50L234,6',
    off: [],
  });
  assert.deepEqual(rateGraphShape({ usedPercent: 100, resetsAt: END, points: [[at(0.5), 60]] }, FIVE_HOURS_MS, END + 3_600_000), {
    path: 'M6,50L120,23.6H234V6',
    dot: { x: 234, y: 6 },
    pace: 100,
    pacePath: 'M6,50L234,6',
    off: [],
  });
});

test('rateGraphShape は、直前の点より前を今にせず、リセットの時刻が無い値は描かない', () => {
  const points: RatePoint[] = [[at(0.5), 30]];
  assert.deepEqual(rateGraphShape({ usedPercent: 30, resetsAt: END, points }, FIVE_HOURS_MS, at(0.25)), {
    path: 'M6,50L120,36.8H120V36.8',
    dot: { x: 120, y: 36.8 },
    pace: 50,
    pacePath: 'M6,50L234,6',
    off: [],
  });
  assert.equal(rateGraphShape({ usedPercent: 30 }, FIVE_HOURS_MS, at(0.5)), undefined);
});

test('rateGraphShape は、平日だけで数えるとき、土日祝日を続けて 1 つの区間にし、その間はペースを増やさない', () => {
  // 2026-10-09（金）17:00 からの 7 日。10/10（土）から 10/12（月、スポーツの日）までが休みで、平日は 96 時間ある。
  const end = new Date(2026, 9, 16, 17).getTime();
  const shape = rateGraphShape({ usedPercent: 30, resetsAt: end }, SEVEN_DAYS_MS, new Date(2026, 9, 14, 17).getTime(), true);
  assert.deepEqual(shape?.off, [{ x: 15.5, width: 97.7 }]);
  assert.equal(shape?.pacePath, 'M6,50L15.5,46.8L113.2,46.8L234,6');
  // 平日の 48 時間が過ぎたところ。
  assert.equal(shape?.pace, 50);
});

test('paceOf は、均等なペースに対する使用率の比を返し、使っていなければ 0、経過がほぼ 0 なら 999 で止める', () => {
  assert.equal(paceOf(30, 60), 50);
  assert.equal(paceOf(90, 60), 150);
  assert.equal(paceOf(0, 0), 0);
  assert.equal(paceOf(5, 0), 999);
});
