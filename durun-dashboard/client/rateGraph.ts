import type { RateLimit, RatePoint } from '../shared/types.ts';

const HOUR_MS = 60 * 60 * 1000;
export const FIVE_HOURS_MS = 5 * HOUR_MS;
export const SEVEN_DAYS_MS = 7 * 24 * HOUR_MS;

export const GRAPH_WIDTH = 240;
export const GRAPH_HEIGHT = 56;
export const DOT_RADIUS = 4;
const RING_WIDTH = 2;
// 縁取りは塗りの下に描くので、線の太さは見せたい幅の 2 倍にする。
export const RING_STROKE = RING_WIDTH * 2;
// 今の位置の印は、縁取りまで含めて四角の角に置かれても欠けないよう、四角の外に印の分の余白を取る。
const PAD = DOT_RADIUS + RING_WIDTH;
/** 横軸の枠の始まりからリセットまで、縦軸の 0〜100% を割り当てる四角 */
export const PLOT = { left: PAD, right: GRAPH_WIDTH - PAD, top: PAD, bottom: GRAPH_HEIGHT - PAD };

export type RateGraphShape = {
  /** 枠の始まりの 0% から最初の点までを直線で結び、その先は使用率が変わった時点で折れる階段にして、今の使用率まで引く線 */
  path: string;
  /** 今の位置 */
  dot: { x: number; y: number };
  /** 今の時点で、枠の始まりからリセットまで均等に使ってきたときの使用率 */
  pace: number;
};

const unit = (ratio: number): number => Math.min(1, Math.max(0, ratio));
const tenth = (px: number): number => Math.round(px * 10) / 10;

/** 使用率の推移のグラフの座標。枠の終わり（リセット）が分からない値は、枠の始まりも決まらないので描かない。 */
export const rateGraphShape = (limit: RateLimit, windowMs: number, now: number): RateGraphShape | undefined => {
  if (limit.resetsAt === undefined) return undefined;
  const start = limit.resetsAt - windowMs;
  const x = (at: number): number => tenth(PLOT.left + unit((at - start) / windowMs) * (PLOT.right - PLOT.left));
  const y = (percent: number): number => tenth(PLOT.bottom - unit(percent / 100) * (PLOT.bottom - PLOT.top));
  const points = limit.points ?? [];
  // 現在時刻は 30 秒ごとにしか進まず、サーバーの時計ともずれるので、直前に記録した点より前になりうる。そのときは点の時刻を今として、線を左へ戻さない。
  const current = Math.max(now, points.at(-1)?.[0] ?? now);
  const steps: RatePoint[] = [...points, [current, limit.usedPercent]];
  // 記録を始める前から続いていた枠では、最初の点より前の推移が分からない。0% のまま平らに引くと誤るので、直線で結ぶ。
  return {
    path: steps.reduce((d, [at, percent], i) => `${d}${i ? `H${x(at)}V` : `L${x(at)},`}${y(percent)}`, `M${x(start)},${y(0)}`),
    dot: { x: x(current), y: y(limit.usedPercent) },
    pace: unit((current - start) / windowMs) * 100,
  };
};
