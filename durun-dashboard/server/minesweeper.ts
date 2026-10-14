// マインスイーパのヒント。人間に見えている盤面だけを使い、開いていないマスごとに地雷がある確率を推定する。
// Jev は盤面の図から位置を読み取れず、どのマスにも半々と答える。そこで、数字から確実に決まるマスは先に規則で決め、
// 残りのマスだけを、接している数字の状況を言葉で書き込んで Jev に問う。
import { askNoul } from './jev.ts';

/** 盤面の 1 行を、左のマスから 1 文字ずつ表す。. は開いていないマス、F は旗、0〜8 は開いたマスの周りの地雷の数。 */
export const MINES_ROW = /^[.F0-8]+$/;

/** known は規則で決まったマスで、mine は地雷、safe は安全。 */
type Spot = { c: string; x: number; y: number; known?: 'mine' | 'safe' };

const around = (grid: Spot[][], { x, y }: Spot): Spot[] =>
  [-1, 0, 1].flatMap((dy) => [-1, 0, 1].map((dx) => (dx || dy ? grid[y + dy]?.[x + dx] : undefined))).filter((s): s is Spot => !!s);

const isNumber = (s: Spot): boolean => /\d/.test(s.c);
const isOpen = (s: Spot): boolean => s.c === '.' && !s.known;

/** 数字の周りで、まだ決まっていない地雷の数と、決まっていないマスの数。旗と、規則で地雷と決めたマスは地雷に数える。 */
const needOf = (grid: Spot[][], n: Spot): { left: number; unknown: number } => {
  const near = around(grid, n);
  const mines = near.filter((s) => s.c === 'F' || s.known === 'mine').length;
  return { left: Number(n.c) - mines, unknown: near.filter(isOpen).length };
};

/**
 * 数字 1 つだけで決まるマスを、決まらなくなるまで決めていく。地雷が残っていない数字の周りは安全、
 * 残りの地雷が決まっていないマスと同じ数なら、その周りはすべて地雷。
 */
const deduce = (grid: Spot[][]): void => {
  for (let changed = true; changed; ) {
    changed = false;
    for (const n of grid.flat().filter(isNumber)) {
      const { left, unknown } = needOf(grid, n);
      if (!unknown || (left > 0 && left < unknown)) continue;
      for (const s of around(grid, n).filter(isOpen)) s.known = left <= 0 ? 'safe' : 'mine';
      changed = true;
    }
  }
};

const clueOf = (grid: Spot[][], n: Spot): string => {
  const { left, unknown } = needOf(grid, n);
  return `- 数字 ${n.c} のマス: まだ分からないマス ${unknown} 個のうち ${left} 個が地雷（${Math.round((left / unknown) * 100)}%）`;
};

// Jev の答えは 40〜60% に寄るので、盤面の平均からの差をこの倍率で広げる。
const STRETCH = 4;
const logit = (p: number): number => Math.log(Math.min(0.99, Math.max(0.01, p)) / (1 - Math.min(0.99, Math.max(0.01, p))));
const sigmoid = (v: number): number => 1 / (1 + Math.exp(-v));

/** 開いていないマス（旗を除く）の位置ごとに、地雷がある確率を返す。ほかのマスは null にする。 */
export const minesHint = async (rows: string[], mines: number): Promise<(number | null)[]> => {
  const grid: Spot[][] = rows.map((row, y) => [...row].map((c, x) => ({ c, x, y })));
  const cells = grid.flat();
  deduce(grid);
  const targets = cells.filter(isOpen);
  const odds = new Map<Spot, number>(cells.filter((s) => s.known).map((s) => [s, s.known === 'mine' ? 1 : 0]));
  if (targets.length) {
    const found = cells.filter((s) => s.c === 'F' || s.known === 'mine').length;
    // 残りの地雷を、まだ分からないマスに均した割合。数字に接していないマスの目安にし、Jev の答えを広げる中心にもする。
    const base = Math.min(0.99, Math.max(0.01, (mines - found) / targets.length));
    const raw = await askNoul(
      { 盤面: rows, 地雷の総数: mines, 分かっている地雷の数: found, まだ分からないマスの数: targets.length },
      targets.map((t) => {
        const clues = around(grid, t).filter(isNumber).map((n) => clueOf(grid, n));
        return {
          instructions: [
            'マインスイーパで、このマスに地雷があるかを推理してください。',
            clues.length ? `このマスに接している数字:\n${clues.join('\n')}` : `このマスは数字に接していないので、手がかりは残りの地雷の割合（約 ${Math.round(base * 100)}%）だけです。`,
            '接している数字の割合が高いほど地雷がありそうで、低い割合の数字が 1 つでもあれば地雷は少なそうです。',
          ].join('\n'),
          criteria: { true: 'このマスに地雷がある', false: 'このマスに地雷は無い' },
        };
      }),
    );
    const mean = raw.reduce((a, p) => a + logit(p), 0) / raw.length;
    targets.forEach((t, n) => odds.set(t, sigmoid(logit(base) + STRETCH * (logit(raw[n]!) - mean))));
  }
  return cells.map((s) => odds.get(s) ?? null);
};
