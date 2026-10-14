// マインスイーパのヒント。人間に見えている盤面だけを Jev に渡し、開いていないマスごとに地雷がある確率を推定させる。
import { askNoul } from './jev.ts';

/** 盤面の 1 行を、左のマスから 1 文字ずつ表す。. は開いていないマス、F は旗、0〜8 は開いたマスの周りの地雷の数。 */
export const MINES_ROW = /^[.F0-8]+$/;

const STATE_NOTE = '盤面の各行は、左のマスから 1 文字ずつ表す。. は開いていないマス、F は人間が地雷だと思って旗を立てたマス、0〜8 は開いたマスで、周りの 8 マスにある地雷の数。';

/** 開いていないマス（旗を除く）の位置ごとに、地雷がある確率を返す。ほかのマスは null にする。 */
export const minesHint = async (rows: string[], mines: number): Promise<(number | null)[]> => {
  const cells = rows.flatMap((row, y) => [...row].map((c, x) => ({ c, x, y })));
  const targets = cells.filter((t) => t.c === '.');
  if (!targets.length) return cells.map(() => null);
  const odds = await askNoul(
    { 説明: STATE_NOTE, 盤面: rows, 地雷の総数: mines, 旗の数: cells.filter((t) => t.c === 'F').length },
    targets.map(({ x, y }) => ({
      instructions: `盤面の上から ${y + 1} 行目、左から ${x + 1} 列目のマスに地雷があるかを、開いたマスの数字と地雷の総数から推理してください。`,
      criteria: { true: 'このマスに地雷がある', false: 'このマスに地雷は無い' },
    })),
  );
  const byCell = new Map(targets.map((t, n) => [t, odds[n]!]));
  return cells.map((t) => byCell.get(t) ?? null);
};
