import assert from 'node:assert/strict';
import { test } from 'node:test';
import { glide, pushSample, releaseVelocity, type Sample } from '../client/fling.ts';

/** t0 から ms ごとに、1 回に (dx, dy) ずつ動いたポインタの記録。 */
const stroke = (count: number, ms: number, dx: number, dy: number): Sample[] =>
  Array.from({ length: count }, (_, i) => ({ t: i * ms, x: i * dx, y: i * dy })).reduce<Sample[]>(pushSample, []);

test('勢いよく離したら直前の動きの速さで飛ばし、ゆっくり離すか、止めてから離したら飛ばさない', () => {
  // 10ms ごとに右へ 20px、上へ 10px。秒速にすると右へ 2000、上へ 1000。
  const fast = stroke(8, 10, 20, -10);
  const v = releaseVelocity(fast, 75);
  assert.ok(v);
  assert.equal(Math.round(v.vx), 2000);
  assert.equal(Math.round(v.vy), -1000);

  assert.equal(releaseVelocity(stroke(8, 10, 2, 0), 75), undefined);
  // 同じ速い動きでも、止めて 200ms 待ってから離したら置いたものとする。
  assert.equal(releaseVelocity(fast, 270), undefined);
  assert.equal(releaseVelocity([], 0), undefined);
});

test('速さは直前の 100ms だけから測り、上限で頭打ちにする', () => {
  // 先に左へ大きく振りかぶってから、右へ払う。振りかぶった分は混ぜない。
  const windup: Sample[] = [{ t: 0, x: 500, y: 0 }, { t: 50, x: 0, y: 0 }];
  const swipe = [200, 210, 220, 230, 240, 250].map((t, i) => ({ t, x: i * 10, y: 0 }));
  const v = releaseVelocity([...windup, ...swipe].reduce<Sample[]>(pushSample, []), 255);
  assert.equal(Math.round(v?.vx ?? 0), 1000);

  const capped = releaseVelocity(stroke(8, 10, 200, 0), 75);
  assert.equal(Math.round(Math.hypot(capped?.vx ?? 0, capped?.vy ?? 0)), 3500);
});

test('飛ばしたものは、進む向きを保ったまま減速し、端に当たると弱く跳ね返って、画面の外へ出ない', () => {
  const bounds = { maxX: 1000, maxY: 600 };
  let state = { x: 100, y: 300, vx: 1500, vy: 0 };
  const speeds: number[] = [];
  for (let i = 0; i < 20; i++) {
    state = glide(state.x, state.y, state, 1 / 60, bounds);
    speeds.push(state.vx);
  }
  assert.ok(state.x > 100 && state.vy === 0);
  assert.ok(speeds.every((s, i) => i === 0 || s < (speeds[i - 1] ?? 0)), '毎フレーム遅くなる');

  // 右端のすぐ手前から右へ飛ばすと、端で止まって左向きに、弱くなって返る。
  const hit = glide(990, 300, { vx: 1500, vy: 0 }, 1 / 60, bounds);
  assert.equal(hit.x, 1000);
  assert.ok(hit.vx < 0 && Math.abs(hit.vx) < 1500 / 2);

  // どれだけ速くても、止まるまで画面の中にいる。
  state = { x: 500, y: 300, vx: -3500, vy: 3500 };
  for (let i = 0; i < 600; i++) {
    state = glide(state.x, state.y, state, 1 / 60, bounds);
    assert.ok(state.x >= 0 && state.x <= 1000 && state.y >= 0 && state.y <= 600);
  }
  assert.ok(Math.hypot(state.vx, state.vy) < 1);
});
