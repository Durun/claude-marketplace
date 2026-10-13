// マインスイーパの盤面と規則。描画から切り離し、テストできるようにする。

export type Cell = { mine: boolean; open: boolean; flag: boolean };
/** ready は最初のマスを開く前。XP と同じく、最初に開いたマスとその周りには地雷を置かない。 */
export type Board = { w: number; h: number; mines: number; cells: Cell[]; state: 'ready' | 'play' | 'won' | 'lost' };

/** XP の「初級」の大きさ。 */
export const BEGINNER = { w: 9, h: 9, mines: 10 };

export const newBoard = ({ w, h, mines } = BEGINNER): Board => ({ w, h, mines, cells: Array.from({ length: w * h }, () => ({ mine: false, open: false, flag: false })), state: 'ready' });

export const neighbors = (b: Board, i: number): number[] => {
  const x = i % b.w;
  const y = Math.floor(i / b.w);
  const out: number[] = [];
  for (let dy = -1; dy <= 1; dy++)
    for (let dx = -1; dx <= 1; dx++) {
      const nx = x + dx;
      const ny = y + dy;
      if ((dx || dy) && nx >= 0 && nx < b.w && ny >= 0 && ny < b.h) out.push(ny * b.w + nx);
    }
  return out;
};

export const countAround = (b: Board, i: number): number => neighbors(b, i).filter((n) => b.cells[n]!.mine).length;

export const flagsLeft = (b: Board): number => b.mines - b.cells.filter((c) => c.flag).length;

const placeMines = (b: Board, safe: number, rand: () => number): void => {
  const keep = new Set([safe, ...neighbors(b, safe)]);
  // 盤面が小さく周りまで空けられないときは、最初のマスだけを空ける。
  const free = b.cells.map((_, i) => i).filter((i) => (b.cells.length - keep.size >= b.mines ? !keep.has(i) : i !== safe));
  for (let n = 0; n < b.mines; n++) {
    const [i] = free.splice(Math.floor(rand() * free.length), 1);
    b.cells[i!]!.mine = true;
  }
};

/** マスを開く。周りに地雷が無いマスは、周りも続けて開く。 */
export const reveal = (prev: Board, i: number, rand: () => number = Math.random): Board => {
  if (prev.state === 'won' || prev.state === 'lost' || prev.cells[i]!.open || prev.cells[i]!.flag) return prev;
  const b: Board = structuredClone(prev);
  if (b.state === 'ready') {
    placeMines(b, i, rand);
    b.state = 'play';
  }
  if (b.cells[i]!.mine) {
    b.cells.forEach((c) => c.mine && (c.open = true));
    b.state = 'lost';
    return b;
  }
  const todo = [i];
  while (todo.length) {
    const j = todo.pop()!;
    const c = b.cells[j]!;
    if (c.open || c.flag) continue;
    c.open = true;
    if (countAround(b, j) === 0) todo.push(...neighbors(b, j));
  }
  if (b.cells.every((c) => c.mine || c.open)) b.state = 'won';
  return b;
};

export const toggleFlag = (prev: Board, i: number): Board => {
  if (prev.state === 'won' || prev.state === 'lost' || prev.cells[i]!.open) return prev;
  const b: Board = structuredClone(prev);
  b.cells[i]!.flag = !b.cells[i]!.flag;
  return b;
};
