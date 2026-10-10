// 引っぱって勢いよく離したものを、慣性で滑らせる。画面に触れない計算だけを置く。

/** 引っぱっている間のポインタの位置（px）と時刻（ms）。 */
export type Sample = { t: number; x: number; y: number };

/** 速さ（px/s）。 */
export type Velocity = { vx: number; vy: number };

export type Bounds = { maxX: number; maxY: number };

// 離す直前のこの長さの動きから速さを測る。短すぎると 1 回の揺れに振り回され、長すぎると振りかぶった動きまで混ざる。
const WINDOW_MS = 100;
// 最後に動いてからこれより長く止めて離したら、置いたものとして飛ばさない。
const STILL_MS = 80;
// これより遅く離したら、置いたものとして飛ばさない。
const MIN_SPEED = 350;
// 画面を一瞬で横切らないよう、上限を決める。
const MAX_SPEED = 3500;
// 水の抵抗に見えるよう、速さに比例して減速する。止まるまでに進む距離は 速さ ÷ DRAG になる。
const DRAG = 2.2;
// 画面の端で跳ね返るときに残す速さの割合。水の中なので、弾みは小さくする。
const BOUNCE = 0.45;

/** 直近の動きだけを残す。pointermove のたびに呼ぶ。 */
export const pushSample = (samples: Sample[], sample: Sample): Sample[] => [...samples.filter((s) => sample.t - s.t <= WINDOW_MS), sample];

/** 離した瞬間の速さ。勢いが足りないか、止めてから離したなら undefined。 */
export const releaseVelocity = (samples: Sample[], now: number): Velocity | undefined => {
  const recent = samples.filter((s) => now - s.t <= WINDOW_MS);
  const first = recent[0];
  const last = recent.at(-1);
  if (!first || !last || last.t === first.t || now - last.t > STILL_MS) return undefined;
  const seconds = (last.t - first.t) / 1000;
  const vx = (last.x - first.x) / seconds;
  const vy = (last.y - first.y) / seconds;
  const speed = Math.hypot(vx, vy);
  if (speed < MIN_SPEED) return undefined;
  const scale = Math.min(1, MAX_SPEED / speed);
  return { vx: vx * scale, vy: vy * scale };
};

/** dt 秒ぶん滑らせた位置と速さ。端に当たったら、弾みを弱めて跳ね返す。 */
export const glide = (x: number, y: number, v: Velocity, dt: number, { maxX, maxY }: Bounds): { x: number; y: number } & Velocity => {
  const decay = Math.exp(-DRAG * dt);
  let vx = v.vx * decay;
  let vy = v.vy * decay;
  let nx = x + vx * dt;
  let ny = y + vy * dt;
  if (nx < 0 || nx > maxX) {
    nx = nx < 0 ? 0 : maxX;
    vx = -vx * BOUNCE;
  }
  if (ny < 0 || ny > maxY) {
    ny = ny < 0 ? 0 : maxY;
    vy = -vy * BOUNCE;
  }
  return { x: nx, y: ny, vx, vy };
};
