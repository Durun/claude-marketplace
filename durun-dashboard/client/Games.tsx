// スタートメニューの「ゲーム」から開く、マインスイーパとソリティア。Windows XP のものに倣い、スキンによらず同じ見た目で描く。
// 盤面はウィンドウの設定に JSON で持ち、画面を読み込み直しても続きから遊べるようにする。
import { useEffect, useState } from 'preact/hooks';
import { api } from './api.ts';
import { type Float, FloatFrame, dock, floatUp, latestFloat, setFloatSettings } from './Float.tsx';
import { type Board, countAround, flagsLeft, newBoard, reveal, toggleFlag, visibleRows } from './minesweeper.ts';
import { type Card, type Game, type Pile, RANKS, SUITS, deal, draw, isRed, isWon, move, toFoundation } from './solitaire.ts';

export const MINESWEEPER_KEY = 'minesweeper';
export const MINESWEEPER_TITLE = 'マインスイーパ';
export const MINESWEEPER_ICON = '💣';
export const SOLITAIRE_KEY = 'solitaire';
export const SOLITAIRE_TITLE = 'ソリティア';
export const SOLITAIRE_ICON = '🃏';

// ゲームは 1 つずつ開く。もう開いていれば手前に出す。
export const openMinesweeper = (): void => floatUp(MINESWEEPER_KEY, '', { id: MINESWEEPER_KEY }, { w: 260, h: 320 });
export const openSolitaire = (): void => floatUp(SOLITAIRE_KEY, '', { id: SOLITAIRE_KEY }, { w: 560, h: 560 });

const loadState = <T,>(f: Float): T | undefined => {
  try {
    return f.tab.settings?.state ? (JSON.parse(f.tab.settings.state) as T) : undefined;
  } catch {
    return undefined;
  }
};
const saveState = (f: Float, state: unknown): void => setFloatSettings(f, { ...f.tab.settings, state: JSON.stringify(state) });

/** 保存した盤面を読む。まだ無ければ一度だけ作って保存する。描き直すたびに作ると、ほかのウィンドウを動かすだけで配り直してしまう。 */
const useSaved = <T,>(f: Float, fresh: () => T): T => {
  const saved = loadState<T>(f);
  const [first] = useState(() => saved ?? fresh());
  const missing = saved === undefined;
  useEffect(() => {
    if (missing) saveState(f, first);
  }, [missing]);
  return saved ?? first;
};

const closeOf = (f: Float) => ({ kind: 'close' as const, label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) });

/** Jev が推定した、マスごとの地雷がある確率。開いていないマスのほかは null。rows は推定した時の盤面で、盤面が変わったら出さない。 */
type Hint = { rows: string; odds: (number | null)[] };

const rowsKey = (b: Board): string => visibleRows(b).join('/');

const loadHint = (f: Float, board: Board): Hint | undefined => {
  try {
    const hint = f.tab.settings?.hint ? (JSON.parse(f.tab.settings.hint) as Hint) : undefined;
    return hint?.rows === rowsKey(board) ? hint : undefined;
  } catch {
    return undefined;
  }
};

/** ヒントを出せる盤面か。勝ち負けが決まった後は、出しても役に立たない。 */
export const canHint = (f: Float): boolean => {
  const board = loadState<Board>(f);
  return !board || board.state === 'ready' || board.state === 'play';
};

/** 今の盤面のヒントを Jev に推定させ、ウィンドウに残す。待つ間に盤面が変わっていれば、残しても出ない。 */
export const askMinesHint = async (f: Float): Promise<void> => {
  const board = loadState<Board>(f) ?? newBoard();
  const { odds } = await api.minesHint(visibleRows(board), board.mines);
  const cur = latestFloat(f);
  if (cur) setFloatSettings(cur, { ...cur.tab.settings, hint: JSON.stringify({ rows: rowsKey(board), odds } satisfies Hint) });
};

const FACE: Record<Board['state'], string> = { ready: '🙂', play: '🙂', won: '😎', lost: '😵' };

export const MinesweeperFloat = ({ f }: { f: Float }) => {
  const board = useSaved(f, newBoard);
  const set = (b: Board) => b !== board && saveState(f, b);
  const hint = loadHint(f, board);
  return (
    <FloatFrame f={f} title={MINESWEEPER_TITLE} icon={MINESWEEPER_ICON} controls={[closeOf(f)]}>
      <div class="mines">
        <div class="mines-head">
          <output class="mines-count" aria-label="残りの地雷">{String(flagsLeft(board)).padStart(3, '0')}</output>
          <button class="mines-face" title="新しいゲーム" onClick={() => saveState(f, newBoard())}>{FACE[board.state]}</button>
        </div>
        <div class="mines-grid" style={`--w: ${board.w}`} onContextMenu={(e) => e.preventDefault()}>
          {board.cells.map((c, i) => {
            const n = c.open && !c.mine ? countAround(board, i) : 0;
            const odds = hint?.odds[i];
            return (
              <button
                key={i}
                class={`mines-cell${c.open ? ' is-open' : ''}${odds != null ? ' is-hint' : ''}`}
                style={odds != null ? `--odds: ${odds}` : undefined}
                title={odds != null ? `地雷がある確率 ${Math.round(odds * 100)}%` : undefined}
                data-n={n || undefined}
                aria-label={c.open ? (c.mine ? '地雷' : String(n)) : c.flag ? '旗' : '未開封'}
                onClick={() => set(reveal(board, i))}
                onContextMenu={() => set(toggleFlag(board, i))}
              >
                {c.open ? (c.mine ? '💣' : n || '') : c.flag ? '🚩' : ''}
              </button>
            );
          })}
        </div>
      </div>
    </FloatFrame>
  );
};

type Pick = { pile: Pile; index: number };

const CardFace = ({ c, picked, onClick, onDblClick }: { c: Card; picked?: boolean; onClick?: () => void; onDblClick?: () => void }) =>
  c.up ? (
    <button class={`sol-card${isRed(c) ? ' is-red' : ''}${picked ? ' is-picked' : ''}`} onClick={onClick} onDblClick={onDblClick}>
      {RANKS[c.r]}{SUITS[c.s]}
    </button>
  ) : (
    <button class="sol-card is-back" aria-label="裏向きの札" onClick={onClick} />
  );

/** クリックで札を選び、置き先の列か組札をクリックすると動かす。ダブルクリックで組札へ送る。 */
export const SolitaireFloat = ({ f }: { f: Float }) => {
  const game = useSaved(f, () => deal());
  const [pick, setPick] = useState<Pick>();
  const set = (g: Game | undefined) => {
    setPick(undefined);
    if (g) saveState(f, g);
  };
  const choose = (pile: Pile, index: number) => {
    if (!pick) return setPick({ pile, index });
    if (pick.pile === pile && pick.index === index) return setPick(undefined);
    const next = move(game, pick.pile, pick.index, pile);
    if (next) set(next);
    else setPick({ pile, index });
  };
  // 空いた置き場は、選んだ札を置く先にだけなる。
  const drop = (pile: Pile) => pick && set(move(game, pick.pile, pick.index, pile));
  const isPicked = (pile: Pile, index: number) => pick?.pile === pile && index >= pick.index;
  const won = isWon(game);
  return (
    <FloatFrame f={f} title={SOLITAIRE_TITLE} icon={SOLITAIRE_ICON} controls={[closeOf(f)]}>
      <div class="solitaire">
        <div class="solitaire-bar">
          <button class="solitaire-new" onClick={() => set(deal())}>新しいゲーム</button>
          {won && <span>おめでとうございます。すべての札を組札に揃えました。</span>}
        </div>
        <div class="solitaire-row">
          <div class="sol-slot" title="山札をめくる">
            {game.stock.length ? <CardFace c={game.stock.at(-1)!} onClick={() => set(draw(game))} /> : <button class="sol-card is-empty" aria-label="山札を戻す" onClick={() => set(draw(game))}>↻</button>}
          </div>
          <div class="sol-slot">
            {game.waste.length > 0 && <CardFace c={game.waste.at(-1)!} picked={isPicked('waste', game.waste.length - 1)} onClick={() => choose('waste', game.waste.length - 1)} onDblClick={() => set(toFoundation(game, 'waste'))} />}
          </div>
          <div class="sol-slot is-gap" />
          {game.found.map((pile, i) => {
            const p: Pile = `f${i}`;
            return (
              <div key={p} class="sol-slot">
                {pile.length ? <CardFace c={pile.at(-1)!} picked={isPicked(p, pile.length - 1)} onClick={() => choose(p, pile.length - 1)} /> : <button class="sol-card is-empty" aria-label="組札" onClick={() => drop(p)} />}
              </div>
            );
          })}
        </div>
        <div class="solitaire-row">
          {game.tab.map((col, i) => {
            const p: Pile = `t${i}`;
            return (
              <div key={p} class="sol-column">
                {col.length === 0 && <button class="sol-card is-empty" aria-label="空いた列" onClick={() => drop(p)} />}
                {col.map((c, j) => (
                  <CardFace key={j} c={c} picked={isPicked(p, j)} onClick={() => (c.up ? choose(p, j) : drop(p))} onDblClick={() => j === col.length - 1 && set(toFoundation(game, p))} />
                ))}
              </div>
            );
          })}
        </div>
      </div>
    </FloatFrame>
  );
};
