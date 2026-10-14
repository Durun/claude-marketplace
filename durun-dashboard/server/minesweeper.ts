// マインスイーパのヒント。人間に見えている盤面だけを Jev に渡し、開いていないマスごとに地雷がある確率を推定させる。
// Jev は盤面全体の図から、問いで指した位置を読み取れない。そこで、推測のセオリーを前提に置き、問いごとに、そのマスの周りの図と、接している数字の材料を書き込む。
import { askNoul } from './jev.ts';

/** 盤面の 1 行を、左のマスから 1 文字ずつ表す。. は開いていないマス、F は旗、0〜8 は開いたマスの周りの地雷の数。 */
export const MINES_ROW = /^[.F0-8]+$/;

const THEORY = `マインスイーパで、開いていないマスに地雷があるかを推測するときのセオリー。
- 開いたマスの数字は、そのマスを囲む 8 マス（盤の端では 3 マスか 5 マス）にある地雷の数。
- 数字から、周りの旗の数を引いたものが、周りの開いていないマスに残っている地雷の数。
- 残りの地雷が 0 の数字に接しているマスは安全。
- 残りの地雷の数と、周りの開いていないマスの数が同じ数字に接しているマスは、地雷。
- どちらでもなければ、残りの地雷を周りの開いていないマスの数で割った割合が、目安になる。接している数字が複数あれば、割合が低い数字ほど強い手がかりになる（安全の根拠になる）。
- 隣り合う数字を見比べると決まることがある。例えば、壁際で 1-2-1 と並ぶと両端の 1 の外側が地雷で、真ん中の 2 の外側は安全。1-1 と並ぶと、片方の 1 だけに接するマスは安全なことが多い。
- どの数字にも接していないマスは、残りの地雷の総数を、まだ分からないマスの数で割った割合がおおよその確率になる。数字のそばのマスに地雷が多く要るほど、遠いマスの確率は下がる。`;

type Spot = { c: string; x: number; y: number };

const around = (grid: Spot[][], { x, y }: Spot): Spot[] =>
  [-1, 0, 1].flatMap((dy) => [-1, 0, 1].map((dx) => (dx || dy ? grid[y + dy]?.[x + dx] : undefined))).filter((s): s is Spot => !!s);

/** マスを中心にした 5×5 の図。対象のマスを ?、盤の外を # で表す。 */
const windowOf = (grid: Spot[][], t: Spot): string =>
  [-2, -1, 0, 1, 2].map((dy) => [-2, -1, 0, 1, 2].map((dx) => (dx || dy ? (grid[t.y + dy]?.[t.x + dx]?.c ?? '#') : '?')).join('')).join('\n');

/** t から見た s の向き。隣なら「左下」、2 マス離れていれば「左に 2、下に 1」のように言う。 */
const directionOf = (t: Spot, s: Spot): string => {
  const dx = s.x - t.x;
  const dy = s.y - t.y;
  const lr = dx < 0 ? '左' : dx > 0 ? '右' : '';
  const ud = dy < 0 ? '上' : dy > 0 ? '下' : '';
  if (Math.abs(dx) <= 1 && Math.abs(dy) <= 1) return `${lr}${ud}`;
  return [dx && `${lr}に ${Math.abs(dx)}`, dy && `${ud}に ${Math.abs(dy)}`].filter(Boolean).join('、');
};

/** 接している数字の、推測の材料。結論は書かず、Jev に考えさせる。 */
const clueOf = (grid: Spot[][], n: Spot, t: Spot): string => {
  const near = around(grid, n);
  const flags = near.filter((s) => s.c === 'F').length;
  const unknown = near.filter((s) => s.c === '.').length;
  const spots = near.filter((s) => s.c === '.').map((s) => (s === t ? '?' : directionOf(t, s))).join('、');
  return `- ${directionOf(t, n)}の数字 ${n.c}: 周りの旗 ${flags} 個なので、残りの地雷は ${Number(n.c) - flags} 個。それが、開いていないマス ${unknown} 個（${spots}）のどこかにある`;
};

/** 開いていないマス（旗を除く）の位置ごとに、地雷がある確率を返す。ほかのマスは null にする。 */
export const minesHint = async (rows: string[], mines: number): Promise<(number | null)[]> => {
  const grid = rows.map((row, y) => [...row].map((c, x) => ({ c, x, y })));
  const cells = grid.flat();
  const targets = cells.filter((s) => s.c === '.');
  if (!targets.length) return cells.map(() => null);
  const flags = cells.filter((s) => s.c === 'F').length;
  const odds = await askNoul(
    { セオリー: THEORY, 盤面: rows, 盤面の読み方: '各行は左のマスから 1 文字ずつ。. は開いていないマス、F は旗、0〜8 は開いたマスの数字。', 地雷の総数: mines, 旗の数: flags, 開いていないマスの数: targets.length, 盤面全体の残りの地雷の割合: `${Math.round((Math.max(0, mines - flags) / targets.length) * 100)}%` },
    targets.map((t) => {
      const clues = around(grid, t).filter((s) => /\d/.test(s.c)).map((n) => clueOf(grid, n, t));
      return {
        instructions: [
          'セオリーに沿って、? のマスに地雷があるかを推測してください。',
          `? のマスの周り（5×5、# は盤の外）:\n${windowOf(grid, t)}`,
          clues.length ? `? のマスに接している数字:\n${clues.join('\n')}` : '? のマスは、どの数字にも接していない。',
        ].join('\n\n'),
        criteria: {
          true: '? のマスに地雷がある。残りの地雷が開いていないマスと同じ数の数字に接している、または残りの地雷の割合が高い数字に接している',
          false: '? のマスに地雷は無い。残りの地雷が 0 の数字に接している、または割合の低い数字にしか接していない、またはどの数字にも接しておらず盤面全体の残りの地雷の割合が低い',
        },
      };
    }),
  );
  const byCell = new Map(targets.map((t, n) => [t, odds[n]!]));
  return cells.map((t) => byCell.get(t) ?? null);
};
