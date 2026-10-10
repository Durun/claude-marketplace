import assert from 'node:assert/strict';
import { test } from 'node:test';
import { type Fling, glide, pushSample, releaseVelocity, type Sample, windUp } from '../client/fling.ts';

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
  let state = { x: 100, y: 300, vx: 1500, vy: 0, drag: 2.2 };
  const speeds: number[] = [];
  for (let i = 0; i < 20; i++) {
    state = { ...glide(state.x, state.y, state, 1 / 60, bounds), drag: state.drag };
    speeds.push(state.vx);
  }
  assert.ok(state.x > 100 && state.vy === 0);
  assert.ok(speeds.every((s, i) => i === 0 || s < (speeds[i - 1] ?? 0)), '毎フレーム遅くなる');

  // 右端のすぐ手前から右へ飛ばすと、端で止まって左向きに、弱くなって返る。
  const hit = glide(990, 300, { vx: 1500, vy: 0, drag: 2.2 }, 1 / 60, bounds);
  assert.equal(hit.x, 1000);
  assert.ok(hit.vx < 0 && Math.abs(hit.vx) < 1500 / 2);

  // どれだけ速くても、止まるまで画面の中にいる。
  state = { x: 500, y: 300, vx: -3500, vy: 3500, drag: 2.2 };
  for (let i = 0; i < 600; i++) {
    state = { ...glide(state.x, state.y, state, 1 / 60, bounds), drag: state.drag };
    assert.ok(state.x >= 0 && state.x <= 1000 && state.y >= 0 && state.y <= 600);
  }
  assert.ok(Math.hypot(state.vx, state.vy) < 1);
});

/** 10ms ごとに 1 回 step px ずつ右へ動かし続けたときに、たまる勢い（ms）。 */
const windUpOf = (count: number, step: number, from = 0): number => {
  let ms = from;
  for (let i = 1; i < count; i++) ms = windUp(ms, { t: (i - 1) * 10, x: (i - 1) * step, y: 0 }, { t: i * 10, x: i * step, y: 0 });
  return ms;
};

test('勢いは速く振り回した時間だけたまり、ゆっくり運んでもたまらず、手を止めると失う', () => {
  assert.equal(windUpOf(101, 20), 1000);
  assert.equal(windUpOf(101, 1), 0);
  assert.equal(windUp(0, undefined, { t: 0, x: 0, y: 0 }), 0);
  // ゆっくり運んでいる間は、たまった勢いを保つ。
  assert.equal(windUpOf(101, 1, 1000), 1000);
  // 200ms 止めてから動かすと、たまった勢いは 0 に戻る。
  assert.equal(windUp(1000, { t: 0, x: 0, y: 0 }, { t: 200, x: 500, y: 0 }), 0);
});

test('長く勢いをつけて離すほど減速が弱くなり、同じ速さでも遠くまで飛ぶ', () => {
  const fast = stroke(8, 10, 20, 0);
  const distance = (windUpMs: number): number => {
    const fling = releaseVelocity(fast, 75, windUpMs);
    assert.ok(fling);
    let state: { x: number; y: number } & Fling = { x: 0, y: 0, ...fling };
    for (let i = 0; i < 6000; i++) state = { ...glide(state.x, state.y, state, 1 / 60, { maxX: 1e9, maxY: 1e9 }), drag: fling.drag };
    return state.x;
  };
  const [none, some, full, over] = [0, 1500, 3000, 60000].map(distance) as [number, number, number, number];
  assert.ok(none < some && some < full, `${none} < ${some} < ${full}`);
  assert.ok(full > none * 3, '勢いをつけきると、何倍も遠くまで飛ぶ');
  // つけきった後は、それ以上伸びない。
  assert.equal(over, full);
  // 離した瞬間の速さは、勢いをつけた時間では変わらない。
  assert.equal(releaseVelocity(fast, 75, 3000)?.vx, releaseVelocity(fast, 75)?.vx);
});
