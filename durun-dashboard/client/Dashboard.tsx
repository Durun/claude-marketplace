// ウィジェットを並べたダッシュボード。ホームとセッション概要が、それぞれのウィジェットを渡して使う。
import type { ComponentChildren, ComponentType, JSX, RefObject } from 'preact';
import { useEffect, useLayoutEffect, useRef, useState } from 'preact/hooks';
import { type Control, EDGES, WindowControls, controlsVar, dock, iconVar, floatOut, floatUp, isFloating, useFloats, windowControl } from './Float.tsx';
import {
  COLS,
  ROWS,
  type Layout,
  type Placement,
  type Size,
  type Tab,
  type WidgetSettings,
  clamp,
  dropTab,
  dropTarget,
  fromSaved,
  keepTabs,
  kindOf,
  mergeInto,
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
 * bare は、デスクトップのアイコンのように枠もタイトルバーも描かないウィジェット。ウィンドウには浮かべない。
 */
export type Widget<P> = { title: string; icon: string; Body: ComponentType<WidgetProps<P>>; size: Size; sketch: Sketch; controls?: (data: P) => Control[]; bare?: boolean };
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

const loadLayout = (key: string, widgets: Record<string, unknown>, defaults: Layout): Layout => {
  try {
    const raw = localStorage.getItem(key);
    // 今は無い種類のウィジェットは外す。
    if (raw) return keepTabs(fromSaved(JSON.parse(raw)), (t) => !!widgets[kindOf(t.id)]);
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

const area = (b: Size & { x: number; y: number }): string => `--col: ${b.x + 1} / span ${b.w}; --row: ${b.y + 1} / span ${b.h}`;

/** 升目 1 つ分の幅と高さ。升目の間の隙間を含む。 */
const pitchOf = (grid: HTMLElement): { col: number; row: number } => {
  const style = getComputedStyle(grid);
  return { col: (grid.clientWidth + parseFloat(style.columnGap)) / COLS, row: parseFloat(style.gridAutoRows) + parseFloat(style.rowGap) };
};

type Cell = { x: number; y: number };

const cellAt = (grid: HTMLElement, e: MouseEvent): Cell => {
  const { col, row } = pitchOf(grid);
  const rect = grid.getBoundingClientRect();
  return { x: clamp(Math.floor((e.clientX - rect.left) / col), 0, COLS - 1), y: Math.max(0, Math.floor((e.clientY - rect.top) / row)) };
};

/** 置く先を示す枠。merge は、そこにある枠へタブとして重ねること。 */
type Ghost = Size & Cell & { merge: boolean };

/** tab を cell に置くときの置く先。dropTab と同じく、枠があれば重ね、空いていればグリッドに収まる幅で新しく置く。 */
const ghostOf = (layout: Layout, tab: Tab, cell: Cell, size: Size): Ghost => {
  const target = dropTarget(layout, tab, cell.x, cell.y);
  return target ? { ...target, merge: true } : { ...cell, w: Math.min(size.w, COLS - cell.x), h: size.h, merge: false };
};

const DropGhost = ({ ghost }: { ghost: Ghost }) => (
  <div class="dashboard-ghost" style={area(ghost)} aria-hidden="true">
    <span>{ghost.merge ? 'タブとして重ねる' : 'ここに置く'}</span>
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
 * そのため、重なりから離れると、押し出したウィジェットは元の位置に戻る。
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
      Object.assign(d, { dx, dy });
      update(resizeWidget(d.start, id, dx, dy, edge));
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
 * 見出しのドラッグで枠を動かす操作。押した位置からの移動量を升目の数に丸め、押したときの配置から置き直す。
 * 指した先がほかの枠なら、動かさずに、離したときにタブとして重ねる。
 * タブを並べた枠でタブを押してドラッグしたときは、そのタブだけを離した場所へ移す。空いているところなら新しい枠にする。
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
        m.drop = own ? undefined : dropTab(m.start, m.tab, cell.x, cell.y, size, String(Date.now()));
        setGhost(own ? undefined : ghostOf(m.start, m.tab, cell, size));
        return;
      }
      const target = placementAt(m.start.filter((q) => q.id !== m.p.id), cell.x, cell.y);
      m.drop = target && mergeInto(m.start, m.p.id, target.id);
      setGhost(target && { ...target, merge: true });
      update(target ? m.start : moveWidget(m.start, m.p.id, dx, dy));
    },
    onPointerUp: () => {
      if (move.current?.drop) update(move.current.drop);
      end();
    },
    onLostPointerCapture: end,
  });
  return { ghost, handlers };
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
  widgets: Record<string, { title: string; size: Size; sketch: Sketch }>;
  /** 置く場所を待っているウィジェットの名前 */
  placing?: string;
  onAdd: (kind: string) => void;
  onCancel: () => void;
  onReset: () => void;
  onDone: () => void;
};

const EditBar = ({ widgets, placing, onAdd, onCancel, onReset, onDone }: EditBarProps) => (
  <div class="dashboard-bar">
    {placing === undefined ? (
      <>
        <span id="dashboard-help" class="help">
          見出しをドラッグして動かし、辺や角で大きさを変え、× で外します。ほかのウィジェットの上で離すとタブとして重ね、タブを空いているところへドラッグすると単独に戻します。キーボードではウィジェットを選んで矢印キーで動かし、Shift と矢印キーで大きさを変えます。
        </span>
        <span class="spacer" />
        {/* 一覧の外を押すか Esc で閉じるモーダルで開く。選んだら、置く場所を押してもらう。 */}
        <button class="btn btn-sm widget-add" commandfor="widget-picker" command="show-modal">追加</button>
        <dialog id="widget-picker" class="widget-picker" aria-labelledby="widget-picker-title" closedby="any">
          <div class="widget-picker-head">
            <h2 id="widget-picker-title" class="section-title">ウィジェットを追加</h2>
            <button class="btn btn-sm" commandfor="widget-picker" command="close">閉じる</button>
          </div>
          <div class="widget-picker-list">
            {Object.entries(widgets).map(([kind, w]) => (
              <button key={kind} class="widget-picker-item" commandfor="widget-picker" command="close" onClick={() => onAdd(kind)}>
                <span class="widget-picker-thumb">
                  <WidgetSketch sketch={w.sketch} size={w.size} />
                </span>
                {w.title}
              </button>
            ))}
          </div>
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

type PlacingProps = { grid: RefObject<HTMLElement>; layout: Layout; tab: Tab; title: string; size: Size; onDrop: (next: Layout) => void; onCancel: () => void };

/**
 * 「追加」で選んだウィジェットを置く場所を、グリッドに重ねた透明なボタンで受ける。
 * 押した升目が空いていれば新しく置き、ウィジェットの上ならタブとして重ねる。置く先は、升目に重ねた枠で示す。
 */
const PlacingLayer = ({ grid, layout, tab, title, size, onDrop, onCancel }: PlacingProps) => {
  const [cell, setCell] = useState<Cell>();
  const layer = useRef<HTMLButtonElement>(null);
  useEffect(() => layer.current?.focus(), []);
  const cellOf = (e: MouseEvent): Cell | undefined => (grid.current ? cellAt(grid.current, e) : undefined);
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Escape') return onCancel();
    const step = ARROWS[e.key];
    if (!step) return;
    e.preventDefault();
    setCell((c) => (c ? { x: clamp(c.x + step[0], 0, COLS - 1), y: Math.max(0, c.y + step[1]) } : { x: 0, y: 0 }));
  };
  // キーボードの Enter で押したときは、矢印キーで選んだ升目に置く。
  const onClick = (e: MouseEvent) => {
    const c = e.detail ? cellOf(e) : cell;
    if (c) onDrop(dropTab(layout, tab, c.x, c.y, size, String(Date.now())));
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
      {cell && <DropGhost ghost={ghostOf(layout, tab, cell, size)} />}
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
  /** 編集中だけ渡す。枠そのものに付ける、ドラッグとキーボードの操作。 */
  edit?: JSX.HTMLAttributes<HTMLElement>;
  /** 編集中だけ渡す。タブごとの × で外す。 */
  onRemove?: (id: string) => void;
  /** ウィンドウに浮かべているタブか。 */
  floating: (t: Tab) => boolean;
  /** 編集中は渡さない。選んでいるタブを、浮かぶウィンドウと枠のあいだで切り替える。 */
  onWindow?: (t: Tab, from: DOMRect) => void;
  children?: ComponentChildren;
};

/**
 * 枠に重ねたウィジェットを描く。2 つ以上なら、見出しの代わりにタブを並べ、選んだものを残りの高さいっぱいに出す。
 * 選んでいないタブも隠したまま描いておき、切り替えても書きかけのメッセージやスクロールの位置を残す。
 */
const WidgetGroup = <P extends object>({ p, widgets, data, titleOf, onChange, edit, onRemove, floating, onWindow, children }: GroupProps<P>) => {
  const active = p.tabs.find((t) => t.id === p.active) ?? p.tabs[0];
  const body = (t: Tab) => {
    // 浮かべている間は中身を描かず、点線の枠だけを残す。戻すのはウィンドウの全画面のボタンで行う。
    if (floating(t)) return null;
    const w = widgets[kindOf(t.id)];
    const onSettings = (settings: WidgetSettings) => onChange({ ...p, tabs: p.tabs.map((x) => (x.id === t.id ? { ...x, settings } : x)) });
    return w && <w.Body {...data} settings={t.settings ?? {}} onSettings={onSettings} />;
  };
  // タブを並べた枠では、タブごとの × で外すので、タイトルバー右端には × を置かない。
  const only = p.tabs.length === 1 ? p.tabs[0] : undefined;
  const bare = !!only && !!widgets[kindOf(only.id)]?.bare;
  // ウィンドウに浮かべている間は、ウィジェットのボタンをウィンドウの側に出す。
  const own = active && !floating(active) ? widgets[kindOf(active.id)]?.controls?.(data) : undefined;
  const controls: Control[] = [
    ...(own ?? []),
    ...(onRemove && only ? [{ kind: 'close' as const, label: `${titleOf(only)}を外す`, onClick: () => onRemove(only.id) }] : []),
    ...(onWindow && active && !bare
      ? [windowControl(floating(active), (e: MouseEvent) => onWindow(active, (e.currentTarget as HTMLElement).closest('.widget')!.getBoundingClientRect()))]
      : []),
  ];
  return (
    <section
      class={`widget${active && floating(active) ? ' is-floating' : ''}${bare ? ' is-bare' : ''}`}
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
          {p.tabs.map((t) => (
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
  useEffect(() => setPlacing(undefined), [editing]);
  // リンクで開いた種類のウィジェットを重ねた枠では、そのタブを選ぶ。
  useEffect(() => {
    if (focus) update(layout.map((p) => ({ ...p, active: p.tabs.find((t) => kindOf(t.id) === focus.kind)?.id ?? p.active })));
  }, [focus]);
  const titleOf = (t: Tab): string => t.settings?.title || (widgets[kindOf(t.id)]?.title ?? '');
  const replace = (next: Placement) => update(layout.map((p) => (p.id === next.id ? next : p)));
  // 枠を選んで矢印キーで 1 升ずつ動かし、Shift を押しながらなら大きさを変える。中の入力欄などで押したキーは除く。
  const onArrow = (id: string) => (e: KeyboardEvent) => {
    const step = ARROWS[e.key];
    if (!step || e.target !== e.currentTarget) return;
    e.preventDefault();
    update((e.shiftKey ? resizeWidget : moveWidget)(layout, id, ...step));
  };
  const remove = (id: string) => update(keepTabs(layout, (t) => t.id !== id));
  const placingSize = placing && widgets[kindOf(placing.id)]?.size;
  const floats = useFloats();
  const floating = (t: Tab) => isFloating(floats, storageKey, scope, t.id);
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
  }, [storageKey, scope, layout, floats]);
  const toggleWindow = (t: Tab, from: DOMRect) => (floating(t) ? dock(storageKey, scope, t.id) : floatOut(storageKey, scope, t, from));
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
        {(editing ? layout : readingOrder(layout)).map((p) => (
          <WidgetGroup
            key={p.id}
            p={p}
            widgets={widgets}
            data={data}
            titleOf={titleOf}
            onChange={replace}
            edit={editing ? { tabIndex: 0, 'aria-describedby': 'dashboard-help', onKeyDown: onArrow(p.id), ...move.handlers(p) } : undefined}
            onRemove={editing ? remove : undefined}
            floating={floating}
            onWindow={editing ? undefined : toggleWindow}
          >
            {editing && <span class="widget-resize" aria-hidden="true" {...resize(p.id, 'se')} />}
            {editing && EDGES.filter((edge) => edge !== 'se').map((edge) => <span key={edge} class={`float-edge is-${edge}`} aria-hidden="true" {...resize(p.id, edge)} />)}
          </WidgetGroup>
        ))}
        {editing && move.ghost && <DropGhost ghost={move.ghost} />}
        {editing && placing && placingSize && (
          <PlacingLayer
            grid={grid}
            layout={layout}
            tab={placing}
            title={titleOf(placing)}
            size={placingSize}
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
