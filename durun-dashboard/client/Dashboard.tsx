// ウィジェットを並べたダッシュボード。ホームとセッション概要が、それぞれのウィジェットを渡して使う。
import type { ComponentChildren, ComponentType, JSX, RefObject } from 'preact';
import { useEffect, useLayoutEffect, useRef, useState } from 'preact/hooks';
import { AppIcon, setApps } from './apps.tsx';
import { attachIcon, composerAt } from './attach.ts';
import { type Control, EDGES, WindowControls, controlsVar, dock, iconVar, floatOut, floatUp, useFloats, dockedControls } from './Float.tsx';
import {
  COLS,
  ROWS,
  type Layout,
  type Placement,
  type Size,
  type Tab,
  type WidgetSettings,
  addWidget,
  clamp,
  dropTab,
  dropTarget,
  freeApps,
  fromSaved,
  keepTabs,
  kindOf,
  mergeInto,
  moveTab,
  moveWidget,
  placementAt,
  readingOrder,
  resizeWidget,
} from './layout.ts';

/** settings はこのウィジェットだけの設定。ウィジェットの中で onSettings に渡すと、配置と一緒にブラウザに保存する。 */
export type WidgetProps<P> = P & { settings: WidgetSettings; onSettings: (next: WidgetSettings) => void };
/** 「追加」の一覧に描く、ウィジェットの大まかな見た目。中身を描くと読み込みや問い合わせが走るため、線と箱の略図で見せる。 */
export type Sketch = 'list' | 'cards' | 'text' | 'chat' | 'code' | 'chart';
/**
 * icon はタイトルバーの左端に添える絵文字。size は、編集の「追加」で空いているところに置くときの大きさ。controls は、タイトルバー右端に並べる、このウィジェットのボタン。
 * launch を持つものは「アプリ」。デスクトップのアイコンのように枠もタイトルバーも描かず、升目に合わせず好きな位置に置く。ウィンドウには浮かべない。
 * launch はアプリを起動する。アイコンのほか、スタートメニューの「アプリ」からも呼ぶ。
 */
export type Widget<P> = { title: string; icon: string; Body: ComponentType<WidgetProps<P>>; size: Size; sketch: Sketch; controls?: (data: P) => Control[]; launch?: (data: P) => void };
export type EditProps = { editing: boolean; onEditDone: () => void };
/** リンクで開いた画面に合わせて選ばせるウィジェットの種類。同じ種類へもう一度移れるよう、移るたびに作り直す。 */
export type WidgetFocus = { kind: string };

/** 見出しに件数を添えたウィジェットの中身。高さはグリッドで決まるので、収まらない分は見出しの下でスクロールする。 */
export const WidgetPanel = ({ title, count, children }: { title: string; count?: number; children: ComponentChildren }) => (
  <>
    <h2 class="section-title">
      {title}
      {count !== undefined && <span class="count-pill">{count}</span>}
    </h2>
    <div class="widget-body">{children}</div>
  </>
);

const loadLayout = (key: string, widgets: Record<string, { launch?: unknown }>, defaults: Layout): Layout => {
  try {
    const raw = localStorage.getItem(key);
    // 今は無い種類のウィジェットは外す。
    if (raw) return freeApps(keepTabs(fromSaved(JSON.parse(raw)), (t) => !!widgets[kindOf(t.id)]), (t) => !!widgets[kindOf(t.id)]?.launch);
  } catch {
    // 読めない配置は捨てて、初期配置を使う。
  }
  return defaults;
};

const saveLayout = (key: string, layout: Layout): void => {
  try {
    localStorage.setItem(key, JSON.stringify(layout));
  } catch {
    // 保存できなくても、開いている間はその配置で使える。
  }
};

/** free の枠は升目に置かず、左上からの升目の数（--x、--y）で位置を決める。 */
const area = (b: Size & { x: number; y: number; free?: boolean }): string =>
  b.free ? `--x: ${b.x}; --y: ${b.y}` : `--col: ${b.x + 1} / span ${b.w}; --row: ${b.y + 1} / span ${b.h}`;

/** 升目 1 つ分の幅と高さ。升目の間の隙間を含む。 */
const pitchOf = (grid: HTMLElement): { col: number; row: number } => {
  const style = getComputedStyle(grid);
  return { col: (grid.clientWidth + parseFloat(style.columnGap)) / COLS, row: parseFloat(style.gridAutoRows) + parseFloat(style.rowGap) };
};

type Cell = { x: number; y: number };

const cellAt = (grid: HTMLElement, e: MouseEvent): Cell => {
  const { col, row } = pitchOf(grid);
  const rect = grid.getBoundingClientRect();
  return { x: clamp(Math.floor((e.clientX - rect.left) / col), 0, COLS - 1), y: clamp(Math.floor((e.clientY - rect.top) / row), 0, ROWS - 1) };
};

/** 置く先を示す枠。merge は、そこにある枠へタブとして重ねること。full は、押し出す枠がグリッドの下端をはみ出すので置けないこと。 */
type Ghost = Size & Cell & { merge: boolean; full?: boolean };

/** tab を cell に置くときの置く先。dropTab と同じく、枠があれば重ね、空いていればグリッドに収まる大きさで新しく置く。 */
const ghostOf = (layout: Layout, tab: Tab, cell: Cell, size: Size): Ghost => {
  const target = dropTarget(layout, tab, cell.x, cell.y);
  if (target) return { ...target, merge: true };
  const full = dropTab(layout, tab, cell.x, cell.y, size, "") === layout;
  return { ...cell, w: Math.min(size.w, COLS - cell.x), h: Math.min(size.h, ROWS - cell.y), merge: false, full };
};

const DropGhost = ({ ghost }: { ghost: Ghost }) => (
  <div class={`dashboard-ghost${ghost.full ? ' is-full' : ''}`} style={area(ghost)} aria-hidden="true">
    <span>{ghost.merge ? 'タブとして重ねる' : ghost.full ? '下にはみ出すので置けない' : 'ここに置く'}</span>
  </div>
);

/**
 * グリッドの上端の、ページの先頭からの位置。行の高さは、ここから画面の下端までを等分して決める。
 * 上端はヘッダーと上の帯の高さで動くため、ページの大きさが変わるたびに測り直す。
 * 測った値で行の高さが変わるとページの大きさもまた変わるので、次のフレームで測る。同じフレームで測ると、ResizeObserver が通知を配りきれずにエラーを出す。
 */
const useGridTop = (grid: RefObject<HTMLElement>): number => {
  const [top, setTop] = useState(0);
  useLayoutEffect(() => {
    // 編集の出入りで、グリッドの要素が作り直されることがある。そのため、測るたびに ref から読む。
    const measure = () => {
      const el = grid.current;
      if (el) setTop(el.getBoundingClientRect().top + scrollY);
    };
    measure();
    const observer = new ResizeObserver(() => requestAnimationFrame(measure));
    observer.observe(document.documentElement);
    return () => observer.disconnect();
  }, []);
  return top;
};

type Drag = { start: Layout; x: number; y: number; col: number; row: number; dx: number; dy: number };

/**
 * 辺や角のドラッグで大きさを変える操作。押した位置からの移動量を升目の数に丸め、押したときの配置から置き直す。
 * そのため、重なりから離れると、押し出したウィジェットは元の位置に戻る。押し出した枠がグリッドの下端をはみ出す大きさにはせず、最後に収まった大きさで止める。
 */
const useResize = (grid: RefObject<HTMLElement>, layout: Layout, update: (next: Layout) => void) => {
  const drag = useRef<Drag>();
  return (id: string, edge: string) => ({
    onPointerDown: (e: PointerEvent) => {
      if (e.button !== 0 || !grid.current) return;
      // ドラッグ中にページをスクロールしても升目がずれないよう、ページ上の座標で測る。
      drag.current = { start: layout, x: e.pageX, y: e.pageY, ...pitchOf(grid.current), dx: 0, dy: 0 };
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    },
    onPointerMove: (e: PointerEvent) => {
      const d = drag.current;
      if (!d) return;
      const dx = Math.round((e.pageX - d.x) / d.col);
      const dy = Math.round((e.pageY - d.y) / d.row);
      if (dx === d.dx && dy === d.dy) return;
      const next = resizeWidget(d.start, id, dx, dy, edge);
      if (next === d.start) return;
      Object.assign(d, { dx, dy });
      update(next);
    },
    onLostPointerCapture: () => {
      drag.current = undefined;
    },
  });
};

// 押してからこの距離（px）だけ動かすまではドラッグにしない。タブの見出しを押して選べるようにするため。
const DRAG_START = 4;

type Move = { start: Layout; p: Placement; tab?: Tab; x: number; y: number; dragging: boolean; at: string; drop?: Layout };

/**
 * 指した位置が枠のタブの列の上なら、ドラッグしている tab を入れる位置を、ほかのタブのうち前から数えた数で返す。
 * ほかのタブの中央と比べるので、幅の違うタブを越えた直後に元の位置へ戻らない。
 */
const tabIndexAt = (widget: HTMLElement, tab: string, e: PointerEvent): number | undefined => {
  const strip = widget.querySelector('.widget-tabs');
  if (!strip) return undefined;
  const r = strip.getBoundingClientRect();
  if (e.clientX < r.left || e.clientX >= r.right || e.clientY < r.top || e.clientY >= r.bottom) return undefined;
  return [...strip.querySelectorAll<HTMLElement>('.widget-tab')].filter((el) => {
    const b = el.getBoundingClientRect();
    return el.dataset.tab !== tab && b.left + b.width / 2 < e.clientX;
  }).length;
};

/**
 * 見出しのドラッグで枠を動かす操作。押した位置からの移動量を升目の数に丸め、押したときの配置から置き直す。
 * 指した先がほかの枠なら、動かさずに、離したときにタブとして重ねる。
 * タブを並べた枠でタブを押してドラッグしたときは、そのタブだけを離した場所へ移す。空いているところなら新しい枠にする。
 * 同じ枠のタブの列の上では、移さずに、指した位置へ並べ替える。
 * 押し出した枠がグリッドの下端をはみ出す位置には動かさず、最後に収まった位置で止める。
 */
const useMove = (grid: RefObject<HTMLElement>, layout: Layout, update: (next: Layout) => void, sizeOf: (t: Tab) => Size | undefined) => {
  const move = useRef<Move>();
  const [ghost, setGhost] = useState<Ghost>();
  const end = () => {
    move.current = undefined;
    setGhost(undefined);
  };
  const handlers = (p: Placement) => ({
    onPointerDown: (e: PointerEvent) => {
      const el = e.target as HTMLElement;
      // 見出しの中のボタンや入力欄は、そのまま操作できるようにする。
      const head = el.closest('.widget > :first-child');
      const tabEl = el.closest<HTMLElement>('.widget-tab');
      if (e.button !== 0 || head?.parentElement !== e.currentTarget || (!tabEl && el.closest('button, a, input, textarea, select'))) return;
      const tab = p.tabs.length > 1 ? p.tabs.find((t) => t.id === tabEl?.dataset.tab) : undefined;
      move.current = { start: layout, p, tab, x: e.pageX, y: e.pageY, dragging: false, at: '' };
    },
    onPointerMove: (e: PointerEvent) => {
      const m = move.current;
      if (!m || !grid.current) return;
      if (!(e.buttons & 1)) return end();
      if (!m.dragging) {
        if (Math.hypot(e.pageX - m.x, e.pageY - m.y) < DRAG_START) return;
        // 動かし始めてから捕まえる。押してすぐ捕まえると、クリックがタブに届かず選べない。
        m.dragging = true;
        (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
      }
      const index = m.tab && tabIndexAt(e.currentTarget as HTMLElement, m.tab.id, e);
      if (m.tab && index !== undefined) {
        // 列を出たときに置く先を測り直すよう、覚えた升目を忘れる。
        m.at = '';
        m.drop = undefined;
        setGhost(undefined);
        if (p.tabs[index]?.id !== m.tab.id) update(moveTab(layout, m.tab.id, index));
        return;
      }
      const cell = cellAt(grid.current, e);
      const { col, row } = pitchOf(grid.current);
      const dx = Math.round((e.pageX - m.x) / col);
      const dy = Math.round((e.pageY - m.y) / row);
      const at = `${cell.x},${cell.y},${dx},${dy}`;
      if (at === m.at) return;
      m.at = at;
      if (m.tab) {
        const size = sizeOf(m.tab) ?? m.p;
        const own = placementAt([m.p], cell.x, cell.y);
        const drop = own ? undefined : dropTab(m.start, m.tab, cell.x, cell.y, size, String(Date.now()));
        m.drop = drop === m.start ? undefined : drop;
        setGhost(own ? undefined : ghostOf(m.start, m.tab, cell, size));
        return;
      }
      const target = placementAt(m.start.filter((q) => q.id !== m.p.id), cell.x, cell.y);
      m.drop = target && mergeInto(m.start, m.p.id, target.id);
      setGhost(target && { ...target, merge: true });
      const next = target ? m.start : moveWidget(m.start, m.p.id, dx, dy);
      if (target || next !== m.start) update(next);
    },
    onPointerUp: () => {
      if (move.current?.drop) update(move.current.drop);
      end();
    },
    onLostPointerCapture: end,
  });
  return { ghost, handlers };
};

/** composer は、アイコンを重ねている送信欄。 */
type FreeMove = { p: Placement; start: Layout; x: number; y: number; dragging: boolean; next?: Layout; composer?: HTMLElement };

/**
 * アプリのアイコンをドラッグで動かす操作。升目に丸めず、押した位置からの移動量のまま動かす。編集中でなくても動かせる。
 * 動かしている間は show で描くだけにし、離したときに save で保存する。
 * 書き足す値を持つアイコンを送信欄の上で離したときは、メッセージに添付し、アイコンは動かす前の位置に戻す。
 */
const useFreeMove = (grid: RefObject<HTMLElement>, layout: Layout, show: (next: Layout) => void, save: (next: Layout) => void) => {
  const move = useRef<FreeMove>();
  const over = (m: FreeMove, composer?: HTMLElement) => {
    if (m.composer === composer) return;
    m.composer?.classList.remove('is-drop');
    composer?.classList.add('is-drop');
    m.composer = composer;
  };
  const end = () => {
    if (move.current) over(move.current);
    move.current = undefined;
  };
  return (p: Placement) => ({
    onPointerDown: (e: PointerEvent) => {
      // 編集中の × は、そのまま押せるようにする。
      if (e.button !== 0 || (e.target as HTMLElement).closest('.window-controls')) return;
      move.current = { p, start: layout, x: e.pageX, y: e.pageY, dragging: false };
    },
    onPointerMove: (e: PointerEvent) => {
      const m = move.current;
      if (!m || !grid.current) return;
      if (!(e.buttons & 1)) return end();
      const el = e.currentTarget as HTMLElement;
      if (!m.dragging) {
        if (Math.hypot(e.pageX - m.x, e.pageY - m.y) < DRAG_START) return;
        // 動かし始めてから捕まえる。押してすぐ捕まえると、ダブルクリックがアイコンに届かず起動できない。
        m.dragging = true;
        el.setPointerCapture(e.pointerId);
      }
      const { col, row } = pitchOf(grid.current);
      // ダッシュボードの外へは出さない。下は、ウィジェットが無くてもグリッドの下端まで置けるようにする。
      const x = clamp(m.p.x + (e.pageX - m.x) / col, 0, (grid.current.clientWidth - el.offsetWidth) / col);
      const y = clamp(m.p.y + (e.pageY - m.y) / row, 0, ROWS - el.offsetHeight / row);
      m.next = layout.map((q) => (q.id === m.p.id ? { ...q, x, y } : q));
      show(m.next);
      over(m, composerAt(el, e.clientX, e.clientY));
    },
    onPointerUp: (e: PointerEvent) => {
      const m = move.current;
      if (m?.composer) {
        attachIcon(e.currentTarget as HTMLElement, m.composer);
        show(m.start);
      } else if (m?.next) save(m.next);
      end();
    },
    onLostPointerCapture: end,
  });
};

const SKETCHES: Record<Sketch, () => ComponentChildren> = {
  list: () => [70, 55, 80, 45, 65].map((w, i) => <span key={i} class="sketch-row" style={`--w: ${w}%`} />),
  cards: () => [0, 1, 2, 3].map((i) => <span key={i} class="sketch-card" />),
  text: () => [40, 95, 90, 70, 85].map((w, i) => <span key={i} class="sketch-line" style={`--w: ${w}%`} />),
  chat: () => [60, 40, 70, 35].map((w, i) => <span key={i} class="sketch-bubble" style={`--w: ${w}%`} />),
  code: () => (
    <>
      <span class="sketch-tree" />
      <span class="sketch-lines">
        {[50, 75, 60, 85, 40, 70].map((w, i) => <span key={i} class="sketch-line" style={`--w: ${w}%`} />)}
      </span>
    </>
  ),
  chart: () => [40, 65, 50, 85, 70, 95].map((h, i) => <span key={i} class="sketch-bar" style={`--h: ${h}%`} />),
};

/** 置いたときの縦横の比で、略図を描く。 */
const WidgetSketch = ({ sketch, size }: { sketch: Sketch; size: Size }) => (
  <span class={`widget-sketch sketch-${sketch}`} style={`--ratio: ${size.w} / ${size.h}`} aria-hidden="true">
    {SKETCHES[sketch]()}
  </span>
);

type EditBarProps = {
  widgets: Record<string, { title: string; icon: string; size: Size; sketch: Sketch; launch?: unknown }>;
  /** 置く場所を待っているウィジェットの名前 */
  placing?: string;
  onAdd: (kind: string) => void;
  onCancel: () => void;
  onReset: () => void;
  onDone: () => void;
};

// 起動できるものは「アプリ」として、ウィジェットと分けて並べる。
const pickerGroups = (widgets: EditBarProps['widgets']) => {
  const entries = Object.entries(widgets);
  return [
    { name: 'ウィジェット', list: entries.filter(([, w]) => !w.launch) },
    { name: 'アプリ', list: entries.filter(([, w]) => w.launch) },
  ].filter((g) => g.list.length > 0);
};

const EditBar = ({ widgets, placing, onAdd, onCancel, onReset, onDone }: EditBarProps) => (
  <div class="dashboard-bar">
    {placing === undefined ? (
      <>
        <span id="dashboard-help" class="help">
          見出しをドラッグして動かし、辺や角で大きさを変え、× で外します。ほかのウィジェットの上で離すとタブとして重ねます。タブは、並びの中でドラッグすると並べ替え、空いているところへドラッグすると単独に戻します。キーボードではウィジェットを選んで矢印キーで動かし、Shift と矢印キーで大きさを変えます。
        </span>
        <span class="spacer" />
        {/* 一覧の外を押すか Esc で閉じるモーダルで開く。選んだら、置く場所を押してもらう。 */}
        <button class="btn btn-sm widget-add" commandfor="widget-picker" command="show-modal">追加</button>
        <dialog id="widget-picker" class="widget-picker" aria-labelledby="widget-picker-title" closedby="any">
          <div class="widget-picker-head">
            <h2 id="widget-picker-title" class="section-title">ウィジェットを追加</h2>
            <button class="btn btn-sm" commandfor="widget-picker" command="close">閉じる</button>
          </div>
          {pickerGroups(widgets).map((g) => (
            <section key={g.name} class="widget-picker-group" aria-label={g.name}>
              <h3 class="widget-picker-category">{g.name}</h3>
              <div class="widget-picker-list">
                {g.list.map(([kind, w]) => (
                  <button key={kind} class="widget-picker-item" commandfor="widget-picker" command="close" onClick={() => onAdd(kind)}>
                    <span class="widget-picker-thumb">
                      {w.launch ? <AppIcon icon={w.icon} size={32} /> : <WidgetSketch sketch={w.sketch} size={w.size} />}
                    </span>
                    {w.title}
                  </button>
                ))}
              </div>
            </section>
          ))}
        </dialog>
        <button class="btn btn-sm" onClick={onReset}>初期配置に戻す</button>
        <button class="btn btn-sm btn-primary" onClick={onDone}>完了</button>
      </>
    ) : (
      <>
        <span id="dashboard-help" class="help">
          「{placing}」を置く場所を押します。空いているところなら新しく置き、ウィジェットの上ならタブとして重ねます。キーボードでは矢印キーで升目を選び、Enter で置きます。
        </span>
        <span class="spacer" />
        <button class="btn btn-sm" onClick={onCancel}>やめる</button>
      </>
    )}
  </div>
);

const ARROWS: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };

type PlacingProps = { grid: RefObject<HTMLElement>; layout: Layout; tab: Tab; title: string; size: Size; free: boolean; onDrop: (next: Layout) => void; onCancel: () => void };

/**
 * 「追加」で選んだウィジェットを置く場所を、グリッドに重ねた透明なボタンで受ける。
 * 押した升目が空いていれば新しく置き、ウィジェットの上ならタブとして重ねる。置く先は、升目に重ねた枠で示す。
 * free のもの（アプリのアイコン）は、ウィジェットの上でも重ねずに、そこへ置く。
 */
const PlacingLayer = ({ grid, layout, tab, title, size, free, onDrop, onCancel }: PlacingProps) => {
  const [cell, setCell] = useState<Cell>();
  const layer = useRef<HTMLButtonElement>(null);
  useEffect(() => layer.current?.focus(), []);
  const cellOf = (e: MouseEvent): Cell | undefined => (grid.current ? cellAt(grid.current, e) : undefined);
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Escape') return onCancel();
    const step = ARROWS[e.key];
    if (!step) return;
    e.preventDefault();
    setCell((c) => (c ? { x: clamp(c.x + step[0], 0, COLS - 1), y: clamp(c.y + step[1], 0, ROWS - 1) } : { x: 0, y: 0 }));
  };
  // キーボードの Enter で押したときは、矢印キーで選んだ升目に置く。
  const onClick = (e: MouseEvent) => {
    const c = e.detail ? cellOf(e) : cell;
    if (!c) return;
    const id = String(Date.now());
    // アイコンは、下端をはみ出さない高さまで上げて置く。ウィジェットは、押し出す枠が下端をはみ出すなら置かずに待つ。
    const next = free ? addWidget(layout, { id, ...c, y: Math.min(c.y, ROWS - size.h), ...size, tabs: [tab], free }) : dropTab(layout, tab, c.x, c.y, size, id);
    if (next !== layout) onDrop(next);
  };
  return (
    <>
      <button
        ref={layer}
        class="dashboard-placing"
        aria-label={`${title}を置く場所`}
        aria-describedby="dashboard-help"
        onPointerMove={(e) => setCell(cellOf(e))}
        onPointerLeave={() => setCell(undefined)}
        onKeyDown={onKeyDown}
        onClick={onClick}
      />
      {cell && <DropGhost ghost={free ? { ...cell, ...size, merge: false } : ghostOf(layout, tab, cell, size)} />}
    </>
  );
};

const RemoveButton = ({ title, onClick }: { title: string; onClick: () => void }) => (
  <button class="widget-remove" aria-label={`${title}を外す`} title="外す" onClick={onClick}>
    ×
  </button>
);

type GroupProps<P> = {
  p: Placement;
  widgets: Record<string, Widget<P>>;
  data: P;
  titleOf: (t: Tab) => string;
  onChange: (next: Placement) => void;
  /** 枠そのものに付ける、ドラッグとキーボードの操作。編集中の枠と、いつでも動かせるアプリのアイコンに渡す。 */
  edit?: JSX.HTMLAttributes<HTMLElement>;
  /** 編集中だけ渡す。タブごとの × で外す。 */
  onRemove?: (id: string) => void;
  /** ウィンドウに浮かべているタブか。 */
  floating: (t: Tab) => boolean;
  /** 編集中は渡さない。選んでいるタブを、浮かぶウィンドウと枠のあいだで切り替える。 */
  onWindow?: (t: Tab, from: DOMRect, max?: boolean) => void;
  children?: ComponentChildren;
};

/**
 * 枠に重ねたウィジェットを描く。2 つ以上なら、見出しの代わりにタブを並べ、選んだものを残りの高さいっぱいに出す。
 * 選んでいないタブも隠したまま描いておき、切り替えても書きかけのメッセージやスクロールの位置を残す。
 */
const WidgetGroup = <P extends object>({ p, widgets, data, titleOf, onChange, edit, onRemove, floating, onWindow, children }: GroupProps<P>) => {
  const active = p.tabs.find((t) => t.id === p.active) ?? p.tabs[0];
  const body = (t: Tab) => {
    // 浮かべている間は中身を描かず、点線の枠だけを残す。戻すのはウィンドウ化のボタンで行う。
    if (floating(t)) return null;
    const w = widgets[kindOf(t.id)];
    const onSettings = (settings: WidgetSettings) => onChange({ ...p, tabs: p.tabs.map((x) => (x.id === t.id ? { ...x, settings } : x)) });
    return w && <w.Body {...data} settings={t.settings ?? {}} onSettings={onSettings} />;
  };
  // タブを並べた枠では、タブごとの × で外すので、タイトルバー右端には × を置かない。
  const only = p.tabs.length === 1 ? p.tabs[0] : undefined;
  // ウィンドウに浮かべている間は、ウィジェットのボタンをウィンドウの側に出す。
  const own = active && !floating(active) ? widgets[kindOf(active.id)]?.controls?.(data) : undefined;
  const controls: Control[] = [
    ...(own ?? []),
    ...(onRemove && only ? [{ kind: 'close' as const, label: `${titleOf(only)}を外す`, onClick: () => onRemove(only.id) }] : []),
    ...(onWindow && active && !p.free
      ? dockedControls(floating(active), (e, max) => onWindow(active, (e.currentTarget as HTMLElement).closest('.widget')!.getBoundingClientRect(), max))
      : []),
  ];
  return (
    <section
      class={`widget${active && floating(active) ? ' is-floating' : ''}${p.free ? ' is-app' : ''}`}
      data-tabs={p.tabs.map((t) => kindOf(t.id)).join(' ')}
      aria-label={p.tabs.map(titleOf).join('、')}
      style={`${area(p)}; ${controlsVar(controls)}${only ? `; ${iconVar(widgets[kindOf(only.id)]?.icon ?? '')}` : ''}`}
      {...edit}
    >
      {p.tabs.length > 1 ? (
        <>
          <div class="widget-tabs-head">
            <div class="widget-tabs" role="tablist">
              {p.tabs.map((t) => (
                <span key={t.id} class="widget-tab-item">
                  <button class="widget-tab" role="tab" data-tab={t.id} style={iconVar(widgets[kindOf(t.id)]?.icon ?? '')} aria-selected={t === active} onClick={() => onChange({ ...p, active: t.id })}>
                    {titleOf(t)}
                  </button>
                  {onRemove && <RemoveButton title={titleOf(t)} onClick={() => onRemove(t.id)} />}
                </span>
              ))}
            </div>
          </div>
          {/* 要素を動かすと中のスクロールの位置が戻るので、中身はタブの並びでなく id の順に並べ、タブを並べ替えても動かさない。 */}
          {p.tabs.toSorted((a, b) => a.id.localeCompare(b.id)).map((t) => (
            <div key={t.id} class="widget-tab-panel" role="tabpanel" hidden={t !== active}>
              {body(t)}
            </div>
          ))}
        </>
      ) : (
        active && body(active)
      )}
      <WindowControls controls={controls} />
      {children}
    </section>
  );
};

/** 開くよう頼まれたウィジェット。そのダッシュボードがまだ描かれていなければ、描いたときに開く。 */
let request: { storageKey: string; scope: string; kind: string } | undefined;
const requestListeners = new Set<() => void>();

/** ダッシュボードの外から、その種類のウィジェットを見えるようにする。同じものを続けて頼んでも、そのたびに開く。 */
export const showWidget = (storageKey: string, scope: string, kind: string): void => {
  request = { storageKey, scope, kind };
  requestListeners.forEach((fn) => fn());
};

type DashboardProps<P> = EditProps & {
  /** 置けるウィジェットの種類。キーが種類の名前になる。 */
  widgets: Record<string, Widget<P>>;
  defaults: Layout;
  /** 浮かべたウィジェットと、開くよう頼まれたウィジェットを、どの画面のものか区別するキー。 */
  storageKey: string;
  /** 配置を保存する localStorage のキー。省略すると storageKey に保存する。 */
  layoutKey?: string;
  /** すべてのウィジェットに渡す、画面のデータ。 */
  data: P;
  focus?: WidgetFocus;
  /** ウィンドウに浮かべたウィジェットを、どのデータのものか区別する値。セッション画面ならセッションの id。 */
  scope?: string;
};

/** 編集モードで、ウィジェットの位置と大きさを変え、外したり、置いたり、タブとして重ねたりできる。配置はブラウザに保存する。 */
export const Dashboard = <P extends object>({ widgets, defaults, storageKey, layoutKey = storageKey, data, focus, scope = '', editing, onEditDone }: DashboardProps<P>) => {
  const [layout, setLayout] = useState(() => loadLayout(layoutKey, widgets, defaults));
  // 「追加」で選び、置く場所を待っているウィジェット。
  const [placing, setPlacing] = useState<Tab>();
  const grid = useRef<HTMLDivElement>(null);
  const top = useGridTop(grid);
  const update = (next: Layout) => {
    setLayout(next);
    saveLayout(layoutKey, next);
  };
  const resize = useResize(grid, layout, update);
  const move = useMove(grid, layout, update, (t) => widgets[kindOf(t.id)]?.size);
  const freeMove = useFreeMove(grid, layout, setLayout, update);
  // スタートメニューから起動するときも、そのときのデータを渡す。
  const latest = useRef(data);
  latest.current = data;
  useEffect(() => {
    setApps(Object.entries(widgets).flatMap(([kind, w]) => (w.launch ? [{ kind, title: w.title, icon: w.icon, launch: () => w.launch?.(latest.current) }] : [])));
    return () => setApps([]);
  }, [widgets]);
  useEffect(() => setPlacing(undefined), [editing]);
  // リンクで開いた種類のウィジェットを重ねた枠では、そのタブを選ぶ。
  useEffect(() => {
    if (focus) update(layout.map((p) => ({ ...p, active: p.tabs.find((t) => kindOf(t.id) === focus.kind)?.id ?? p.active })));
  }, [focus]);
  const titleOf = (t: Tab): string => t.settings?.title || (widgets[kindOf(t.id)]?.title ?? '');
  const replace = (next: Placement) => update(layout.map((p) => (p.id === next.id ? next : p)));
  // 枠を選んで矢印キーで 1 升ずつ動かし、Shift を押しながらなら大きさを変える。中の入力欄などで押したキーは除く。アプリのアイコンは大きさを変えない。
  const onArrow = (p: Placement) => (e: KeyboardEvent) => {
    const step = ARROWS[e.key];
    if (!step || e.target !== e.currentTarget) return;
    e.preventDefault();
    update((e.shiftKey && !p.free ? resizeWidget : moveWidget)(layout, p.id, ...step));
  };
  const editOf = (p: Placement): JSX.HTMLAttributes<HTMLElement> | undefined => {
    const drag = p.free ? freeMove(p) : move.handlers(p);
    if (editing) return { tabIndex: 0, 'aria-describedby': 'dashboard-help', onKeyDown: onArrow(p), ...drag };
    return p.free ? drag : undefined;
  };
  const remove = (id: string) => update(keepTabs(layout, (t) => t.id !== id));
  const placingWidget = placing && widgets[kindOf(placing.id)];
  // ウィンドウを動かしたりメモ帳に打ったりするたびに、ダッシュボード全体を描き直さないよう、浮かべているタブの id だけを見る。
  const floatingIds = useFloats((list) => list.filter((f) => f.key === storageKey && f.scope === scope).map((f) => f.tab.id).join('\n'));
  const floating = (t: Tab) => floatingIds.split('\n').includes(t.id);
  // 配置に無いか浮かべているウィジェットはウィンドウで手前に出し、枠にあればそのタブを選んで枠まで送る。
  useEffect(() => {
    const take = () => {
      if (request?.storageKey !== storageKey || request.scope !== scope) return;
      const { kind } = request;
      request = undefined;
      const tab = layout.flatMap((p) => p.tabs).find((t) => kindOf(t.id) === kind);
      if (!tab || floating(tab)) return floatUp(storageKey, scope, tab ?? { id: kind });
      update(layout.map((p) => (p.tabs.includes(tab) ? { ...p, active: tab.id } : p)));
      document.querySelector(`.widget[data-tabs~="${kind}"]`)?.scrollIntoView({ block: 'nearest' });
    };
    take();
    requestListeners.add(take);
    return () => {
      requestListeners.delete(take);
    };
  }, [storageKey, scope, layout, floatingIds]);
  const toggleWindow = (t: Tab, from: DOMRect, max?: boolean) => (floating(t) ? dock(storageKey, scope, t.id) : floatOut(storageKey, scope, t, from, max ? 'dock' : undefined));
  return (
    <>
      {editing && (
        <EditBar
          widgets={widgets}
          placing={placing && titleOf(placing)}
          onAdd={(kind) => setPlacing({ id: `${kind}:${Date.now()}` })}
          onCancel={() => setPlacing(undefined)}
          onReset={() => update(defaults)}
          onDone={onEditDone}
        />
      )}
      {!editing && layout.length === 0 && <p class="empty">ウィジェットがありません。メニューの「ダッシュボードを編集」から追加できます。</p>}
      <div ref={grid} class={`dashboard${editing ? ' is-editing' : ''}`} style={`--cols: ${COLS}; --rows: ${ROWS}; --top: ${top}px`}>
        {/* 編集中は配列の順に描き、動かしている間に要素の並びを変えない。並びが変わると、ドラッグとキーボードのフォーカスが外れる。 */}
        {/* アプリのアイコンは編集中でなくても動かすので、同じ理由で、読む順に並べずに後ろへ置く。 */}
        {(editing ? layout : [...readingOrder(layout.filter((p) => !p.free)), ...layout.filter((p) => p.free)]).map((p) => (
          <WidgetGroup
            key={p.id}
            p={p}
            widgets={widgets}
            data={data}
            titleOf={titleOf}
            onChange={replace}
            edit={editOf(p)}
            onRemove={editing ? remove : undefined}
            floating={floating}
            onWindow={editing ? undefined : toggleWindow}
          >
            {editing && !p.free && <span class="widget-resize" aria-hidden="true" {...resize(p.id, 'se')} />}
            {editing && !p.free && EDGES.filter((edge) => edge !== 'se').map((edge) => <span key={edge} class={`float-edge is-${edge}`} aria-hidden="true" {...resize(p.id, edge)} />)}
          </WidgetGroup>
        ))}
        {editing && move.ghost && <DropGhost ghost={move.ghost} />}
        {editing && placing && placingWidget && (
          <PlacingLayer
            grid={grid}
            layout={layout}
            tab={placing}
            title={titleOf(placing)}
            size={placingWidget.size}
            free={!!placingWidget.launch}
            onDrop={(next) => {
              update(next);
              setPlacing(undefined);
            }}
            onCancel={() => setPlacing(undefined)}
          />
        )}
      </div>
    </>
  );
};
