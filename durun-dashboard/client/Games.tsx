// スタートメニューの「ゲーム」から開く、マインスイーパとソリティア。Windows XP のものに倣い、スキンによらず同じ見た目で描く。
// 盤面はウィンドウの設定に JSON で持ち、画面を読み込み直しても続きから遊べるようにする。
import { useState } from 'preact/hooks';
import { type Float, FloatFrame, dock, floatUp, setFloatSettings } from './Float.tsx';
import { type Board, countAround, flagsLeft, newBoard, reveal, toggleFlag } from './minesweeper.ts';
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

const loadState = <T,>(f: Float, fresh: () => T): T => {
  try {
    return f.tab.settings?.state ? (JSON.parse(f.tab.settings.state) as T) : fresh();
  } catch {
    return fresh();
  }
};
const saveState = (f: Float, state: unknown): void => setFloatSettings(f, { ...f.tab.settings, state: JSON.stringify(state) });

const closeOf = (f: Float) => ({ kind: 'close' as const, label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) });

const FACE: Record<Board['state'], string> = { ready: '🙂', play: '🙂', won: '😎', lost: '😵' };

export const MinesweeperFloat = ({ f }: { f: Float }) => {
  const board = loadState(f, newBoard);
  const set = (b: Board) => b !== board && saveState(f, b);
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
            return (
              <button
                key={i}
                class={`mines-cell${c.open ? ' is-open' : ''}`}
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
    <button class={`card${isRed(c) ? ' is-red' : ''}${picked ? ' is-picked' : ''}`} onClick={onClick} onDblClick={onDblClick}>
      {RANKS[c.r]}{SUITS[c.s]}
    </button>
  ) : (
    <button class="card is-back" aria-label="裏向きの札" onClick={onClick} />
  );

/** クリックで札を選び、置き先の列か組札をクリックすると動かす。ダブルクリックで組札へ送る。 */
export const SolitaireFloat = ({ f }: { f: Float }) => {
  const game = loadState(f, () => deal());
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
          <div class="card-slot" title="山札をめくる">
            {game.stock.length ? <CardFace c={game.stock.at(-1)!} onClick={() => set(draw(game))} /> : <button class="card is-empty" aria-label="山札を戻す" onClick={() => set(draw(game))}>↻</button>}
          </div>
          <div class="card-slot">
            {game.waste.length > 0 && <CardFace c={game.waste.at(-1)!} picked={isPicked('waste', game.waste.length - 1)} onClick={() => choose('waste', game.waste.length - 1)} onDblClick={() => set(toFoundation(game, 'waste'))} />}
          </div>
          <div class="card-slot is-gap" />
          {game.found.map((pile, i) => {
            const p: Pile = `f${i}`;
            return (
              <div key={p} class="card-slot">
                {pile.length ? <CardFace c={pile.at(-1)!} picked={isPicked(p, pile.length - 1)} onClick={() => choose(p, pile.length - 1)} /> : <button class="card is-empty" aria-label="組札" onClick={() => drop(p)} />}
              </div>
            );
          })}
        </div>
        <div class="solitaire-row">
          {game.tab.map((col, i) => {
            const p: Pile = `t${i}`;
            return (
              <div key={p} class="card-column">
                {col.length === 0 && <button class="card is-empty" aria-label="空いた列" onClick={() => drop(p)} />}
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
