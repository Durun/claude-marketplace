// ソリティア（クロンダイク、1 枚めくり）の札と規則。描画から切り離し、テストできるようにする。

/** s は ♠♥♦♣ の順の 0〜3、r は A を 1、K を 13 とする。 */
export type Card = { s: number; r: number; up: boolean };
export type Game = { stock: Card[]; waste: Card[]; found: Card[][]; tab: Card[][] };
/** 札の置き場。f0〜f3 は組札、t0〜t6 は場札の列。 */
export type Pile = 'stock' | 'waste' | `f${number}` | `t${number}`;

export const SUITS = ['♠', '♥', '♦', '♣'];
export const RANKS = ['', 'A', '2', '3', '4', '5', '6', '7', '8', '9', '10', 'J', 'Q', 'K'];
export const isRed = (c: Card): boolean => c.s === 1 || c.s === 2;

export const deal = (rand: () => number = Math.random): Game => {
  const deck = Array.from({ length: 52 }, (_, i) => ({ s: Math.floor(i / 13), r: (i % 13) + 1, up: false }));
  for (let i = deck.length - 1; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    [deck[i], deck[j]] = [deck[j]!, deck[i]!];
  }
  const tab = Array.from({ length: 7 }, (_, n) => deck.splice(0, n + 1));
  tab.forEach((col) => (col.at(-1)!.up = true));
  return { stock: deck, waste: [], found: [[], [], [], []], tab };
};

const pileOf = (g: Game, p: Pile): Card[] => (p === 'stock' ? g.stock : p === 'waste' ? g.waste : (p[0] === 'f' ? g.found : g.tab)[Number(p.slice(1))]!);

/** 山札を 1 枚めくる。山札が尽きていれば、めくった札を裏にして山札に戻す。 */
export const draw = (prev: Game): Game => {
  const g: Game = structuredClone(prev);
  if (g.stock.length) {
    g.waste.push({ ...g.stock.pop()!, up: true });
  } else {
    g.stock = g.waste.reverse().map((c) => ({ ...c, up: false }));
    g.waste = [];
  }
  return g;
};

/** from の index 枚目から上の札を to へ動かせるか。組札へは 1 枚ずつ、場札へは色違いで 1 つ小さい札を重ねる。 */
export const canMove = (g: Game, from: Pile, index: number, to: Pile): boolean => {
  if (from === to || from === 'stock' || to === 'stock' || to === 'waste') return false;
  const src = pileOf(g, from);
  const card = src[index];
  if (!card?.up) return false;
  const top = pileOf(g, to).at(-1);
  if (to[0] === 'f') return index === src.length - 1 && (top ? top.s === card.s && top.r + 1 === card.r : card.r === 1);
  return top ? top.up && isRed(top) !== isRed(card) && top.r - 1 === card.r : card.r === 13;
};

/** 札を動かし、動かした後に場札の列の一番上が裏なら表に返す。動かせなければ undefined を返す。 */
export const move = (prev: Game, from: Pile, index: number, to: Pile): Game | undefined => {
  if (!canMove(prev, from, index, to)) return undefined;
  const g: Game = structuredClone(prev);
  const src = pileOf(g, from);
  pileOf(g, to).push(...src.splice(index));
  const top = src.at(-1);
  if (from[0] === 't' && top) top.up = true;
  return g;
};

/** 一番上の札を、置ける組札へ送る。置けなければ undefined を返す。 */
export const toFoundation = (g: Game, from: Pile): Game | undefined => {
  const index = pileOf(g, from).length - 1;
  for (let i = 0; i < 4; i++) {
    const next = move(g, from, index, `f${i}`);
    if (next) return next;
  }
  return undefined;
};

export const isWon = (g: Game): boolean => g.found.every((f) => f.length === 13);
