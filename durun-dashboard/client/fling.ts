// 引っぱって勢いよく離したものを、慣性で滑らせる。画面に触れない計算だけを置く。

/** 引っぱっている間のポインタの位置（px）と時刻（ms）。 */
export type Sample = { t: number; x: number; y: number };

/** 速さ（px/s）。 */
export type Velocity = { vx: number; vy: number };

/** 飛んでいる間の速さと、減速の強さ（1/s）。止まるまでに進む距離は 速さ ÷ drag になる。 */
export type Fling = Velocity & { drag: number };

export type Bounds = { maxX: number; maxY: number };

/** 画面のどの端か。 */
export type Wall = 'left' | 'right' | 'top' | 'bottom';

// 離す直前のこの長さの動きから速さを測る。短すぎると 1 回の揺れに振り回され、長すぎると振りかぶった動きまで混ざる。
const WINDOW_MS = 100;
// 最後に動いてからこれより長く止めて離したら、置いたものとして飛ばさない。
const STILL_MS = 80;
// これより遅く離したら、置いたものとして飛ばさない。
const MIN_SPEED = 350;
// 画面を一瞬で横切らないよう、上限を決める。
const MAX_SPEED = 3500;
// 水の抵抗に見えるよう、速さに比例して減速する。振り回さずに払って離したときの減速の強さ。
const DRAG = 2.2;
// 減速の強さの下限。0 に近づけると、いつまでも止まらず跳ね返り続ける。
const MIN_DRAG = 0.5;
// 画面の端で跳ね返るときに残す速さの割合。水の中なので、弾みは小さくする。
const BOUNCE = 0.45;

/** 直近の動きだけを残す。pointermove のたびに呼ぶ。 */
export const pushSample = (samples: Sample[], sample: Sample): Sample[] => [...samples.filter((s) => sample.t - s.t <= WINDOW_MS), sample];

/**
 * 引っぱったまま振り回して勢いをつけた道のり（px）。pointermove のたびに、直前の記録と今の記録を渡して足していく。
 * 飛ばせる速さで動いた分だけを数える。ゆっくり運んだ分は数えず、手を止めたら勢いを失ったものとして 0 に戻す。
 */
export const windUp = (px: number, prev: Sample | undefined, next: Sample): number => {
  if (!prev) return 0;
  const gap = next.t - prev.t;
  if (gap > STILL_MS) return 0;
  if (gap <= 0) return px;
  const moved = Math.hypot(next.x - prev.x, next.y - prev.y);
  return (moved / gap) * 1000 < MIN_SPEED ? px : px + moved;
};

/**
 * 飛ばした後の減速の強さ。離した速さ（px/s）だけで決まるいつもの飛ぶ距離に、勢いをつけた道のり（px）を足した分だけ進んで止まるようにする。
 * 短く払っただけならいつもの減速とほぼ変わらず、振り回すほどなめらかに弱くなって遠くまで飛ぶ。
 */
const dragAfter = (speed: number, windUpPx: number): number => Math.max(MIN_DRAG, speed / (speed / DRAG + Math.max(0, windUpPx)));

/** 離した瞬間の速さと、その後の減速の強さ。勢いが足りないか、止めてから離したなら undefined。windUpPx は勢いをつけた道のり。 */
export const releaseVelocity = (samples: Sample[], now: number, windUpPx = 0): Fling | undefined => {
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
  return { vx: vx * scale, vy: vy * scale, drag: dragAfter(speed * scale, windUpPx) };
};

/** dt 秒ぶん滑らせた位置と速さ。端に当たったら、弾みを弱めて跳ね返し、当たった端を wall に返す。 */
export const glide = (x: number, y: number, v: Fling, dt: number, { maxX, maxY }: Bounds): { x: number; y: number; wall?: Wall } & Velocity => {
  const decay = Math.exp(-v.drag * dt);
  let vx = v.vx * decay;
  let vy = v.vy * decay;
  let nx = x + vx * dt;
  let ny = y + vy * dt;
  let wall: Wall | undefined;
  if (nx < 0 || nx > maxX) {
    wall = nx < 0 ? 'left' : 'right';
    nx = nx < 0 ? 0 : maxX;
    vx = -vx * BOUNCE;
  }
  if (ny < 0 || ny > maxY) {
    wall ??= ny < 0 ? 'top' : 'bottom';
    ny = ny < 0 ? 0 : maxY;
    vy = -vy * BOUNCE;
  }
  return { x: nx, y: ny, vx, vy, ...(wall && { wall }) };
};
