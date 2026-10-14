// マインスイーパのヒント。人間に見えている盤面だけを Jev に渡し、開いていないマスごとに地雷がある確率を推定させる。
// Jev は盤面全体の図から、問いで指した位置を読み取れない。そこで、推測のセオリーを前提に置き、問いごとに、そのマスの周りの図と、接している数字の材料を書き込む。
// Jev は英語のほうが精度が高いので、渡す文は英語で書く。
import { askNoul } from './jev.ts';

/** 盤面の 1 行を、左のマスから 1 文字ずつ表す。. は開いていないマス、F は旗、0〜8 は開いたマスの周りの地雷の数。 */
export const MINES_ROW = /^[.F0-8]+$/;

const THEORY = `How to reason about whether an unopened Minesweeper cell hides a mine:
- The number on an opened cell is the count of mines among its 8 surrounding cells (3 or 5 at the edges of the board).
- The number minus the flags around it is how many mines remain among its unopened neighbors.
- A cell next to a number with 0 remaining mines is safe.
- A cell next to a number whose remaining mines equal its count of unopened neighbors is a mine.
- Otherwise, remaining mines divided by unopened neighbors is a rough estimate. When a cell touches several numbers, the number with the lowest ratio is the strongest evidence (it argues for safety).
- Comparing adjacent numbers can settle cells. For example, a 1-2-1 along a wall means the cells beyond both 1s are mines and the cell beyond the 2 is safe. With a 1-1 pair, a cell touched by only one of the 1s is often safe.
- A cell that touches no number has roughly the board's remaining mines divided by the unknown cells. The more mines the cells near numbers need, the lower this chance gets.`;

type Spot = { c: string; x: number; y: number };

const around = (grid: Spot[][], { x, y }: Spot): Spot[] =>
  [-1, 0, 1].flatMap((dy) => [-1, 0, 1].map((dx) => (dx || dy ? grid[y + dy]?.[x + dx] : undefined))).filter((s): s is Spot => !!s);

/** マスを中心にした 5×5 の図。対象のマスを ?、盤の外を # で表す。 */
const windowOf = (grid: Spot[][], t: Spot): string =>
  [-2, -1, 0, 1, 2].map((dy) => [-2, -1, 0, 1, 2].map((dx) => (dx || dy ? (grid[t.y + dy]?.[t.x + dx]?.c ?? '#') : '?')).join('')).join('\n');

/** t から見た s の向き。隣なら「upper left」、2 マス離れていれば「2 left, 1 down」のように言う。 */
const directionOf = (t: Spot, s: Spot): string => {
  const dx = s.x - t.x;
  const dy = s.y - t.y;
  const lr = dx < 0 ? 'left' : dx > 0 ? 'right' : '';
  const ud = dy < 0 ? 'up' : dy > 0 ? 'down' : '';
  if (Math.abs(dx) <= 1 && Math.abs(dy) <= 1) return dx && dy ? `${dy < 0 ? 'upper' : 'lower'} ${lr}` : lr || ud;
  return [dx && `${Math.abs(dx)} ${lr}`, dy && `${Math.abs(dy)} ${ud}`].filter(Boolean).join(', ');
};

const plural = (n: number, word: string): string => `${n} ${word}${n === 1 ? '' : 's'}`;

/** 接している数字の、推測の材料。結論は書かず、Jev に考えさせる。 */
const clueOf = (grid: Spot[][], n: Spot, t: Spot): string => {
  const near = around(grid, n);
  const flags = near.filter((s) => s.c === 'F').length;
  const open = near.filter((s) => s.c === '.');
  const spots = open.map((s) => (s === t ? '?' : directionOf(t, s))).join('; ');
  const left = Number(n.c) - flags;
  return `- The ${n.c} to the ${directionOf(t, n)}: ${plural(flags, 'flag')} around it, so ${plural(left, 'mine')} ${left === 1 ? 'remains' : 'remain'} among its ${plural(open.length, 'unopened cell')} (${spots}).`;
};

/** 開いていないマス（旗を除く）の位置ごとに、地雷がある確率を返す。ほかのマスは null にする。 */
export const minesHint = async (rows: string[], mines: number): Promise<(number | null)[]> => {
  const grid = rows.map((row, y) => [...row].map((c, x) => ({ c, x, y })));
  const cells = grid.flat();
  const targets = cells.filter((s) => s.c === '.');
  if (!targets.length) return cells.map(() => null);
  const flags = cells.filter((s) => s.c === 'F').length;
  const odds = await askNoul(
    {
      theory: THEORY,
      board: rows,
      legend: 'Each row lists cells from left to right. "." is unopened, "F" is a flag, and 0-8 are opened cells with their numbers.',
      total_mines: mines,
      flags,
      unopened_cells: targets.length,
      remaining_mine_ratio: `${Math.round((Math.max(0, mines - flags) / targets.length) * 100)}%`,
    },
    targets.map((t) => {
      const clues = around(grid, t).filter((s) => /\d/.test(s.c)).map((n) => clueOf(grid, n, t));
      return {
        instructions: [
          'Following the theory, judge whether the cell marked ? hides a mine.',
          `The 5x5 area around ? ("#" is outside the board):\n${windowOf(grid, t)}`,
          clues.length ? `Numbers touching ?:\n${clues.join('\n')}` : 'The ? cell touches no number.',
        ].join('\n\n'),
        criteria: {
          true: 'The ? cell hides a mine: it touches a number whose remaining mines equal its unopened cells, or it touches numbers with a high ratio of remaining mines.',
          false: 'The ? cell is safe: it touches a number with 0 remaining mines, or only numbers with a low ratio, or it touches no number and the board-wide remaining mine ratio is low.',
        },
      };
    }),
  );
  const byCell = new Map(targets.map((t, n) => [t, odds[n]!]));
  return cells.map((t) => byCell.get(t) ?? null);
};
