// ダッシュボードのウィジェットを、画面の上に浮かぶウィンドウにする。ウィンドウは画面を移っても出したままにし、ブラウザに保存する。
import type { ComponentChildren } from 'preact';
import { createPortal } from 'preact/compat';
import { useEffect, useRef, useState } from 'preact/hooks';
import type { Widget } from './Dashboard.tsx';
import { type Tab, type WidgetSettings, clamp, kindOf, resizeRect } from './layout.ts';

/**
 * 浮かべたウィジェット。key は元のダッシュボードの保存キー、scope はセッション画面ならセッションの id で、ホームなら空にする。
 * tab は元の枠のタブの写しで、ウィンドウで変えた設定は元の枠には戻さない。x, y, w, h は画面上の位置と大きさ（px）。
 * min は最小化してタスクバーにしまっているか。タスクバーがあるのは XP のスキンだけなので、ほかのスキンでは出したままにする。
 * max は最大化しているときに持ち、最大化する前の状態を表す。float はウィンドウ、dock は元の枠に入れた状態。最大化の間も x, y, w, h は元のまま残す。
 */
export type Float = { key: string; scope: string; tab: Tab; x: number; y: number; w: number; h: number; min?: boolean; max?: 'float' | 'dock' };

const STORAGE_KEY = 'dashboard-floats';
// 小さくしすぎて、タイトルバーのボタンを押せなくならない大きさ。
const MIN_W = 240;
const MIN_H = 120;
// タイトルバーを画面の外へ出しきらず、つかんで戻せるよう残す幅（px）。
const KEEP = 80;

/** タイトルバーを今の画面の中に戻す。広い画面で置いたウィンドウが、狭い画面で外に出たままになるのを防ぐ。 */
const inView = (f: Float): Float => ({ ...f, x: clamp(f.x, KEEP - f.w, innerWidth - KEEP), y: clamp(f.y, 0, innerHeight - KEEP) });

const load = (): Float[] => {
  try {
    return (JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '[]') as Float[]).map(inView);
  } catch {
    return [];
  }
};

let floats = load();
const listeners = new Set<(list: Float[]) => void>();

const setFloats = (next: Float[]): void => {
  floats = next;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    // 保存できなくても、開いている間はウィンドウを使える。
  }
  listeners.forEach((fn) => fn(next));
};

/**
 * 浮かべたウィジェットの一覧。select を渡すと一覧から選んだ値を返し、その値が変わったときだけ描き直す。
 * 一覧はウィンドウを動かしたりメモ帳に打ったりするたびに変わるので、浮かべているかだけを見る画面は、比べられる値を選んで描き直しを減らす。
 */
export const useFloats = <T = Float[],>(select: (list: Float[]) => T = (list) => list as T): T => {
  const value = select(floats);
  const [, rerender] = useState(0);
  const latest = useRef({ select, value });
  latest.current = { select, value };
  useEffect(() => {
    const changed = (list: Float[]) => !Object.is(latest.current.select(list), latest.current.value) && rerender((n) => n + 1);
    listeners.add(changed);
    return () => {
      listeners.delete(changed);
    };
  }, []);
  return value;
};

const same = (f: Float, key: string, scope: string, tabId: string): boolean => f.key === key && f.scope === scope && f.tab.id === tabId;

export const isFloating = (list: Float[], key: string, scope: string, tabId: string): boolean => list.some((f) => same(f, key, scope, tabId));

// 浮かべるときに、元の枠からずらす幅と、画面の右と下に空ける幅（px）。下はタスクバーの分も空ける。
const OFFSET = 24;
const MARGIN = 40;

/** 枠の今の位置から少しずらし、画面に収まる大きさで浮かべる。max を渡すと、元の枠から最大化して出す。 */
export const floatOut = (key: string, scope: string, tab: Tab, from: DOMRect, max?: 'dock'): void => {
  const x = from.x + OFFSET;
  const y = from.y + OFFSET;
  const w = Math.max(MIN_W, Math.min(from.width, innerWidth - x - MARGIN));
  const h = Math.max(MIN_H, Math.min(from.height, innerHeight - y - MARGIN));
  setFloats([...floats, { key, scope, tab, x, y, w, h, max }]);
};

/** 同じものを浮かべていれば手前に出し、無ければ画面の中ほどに浮かべる。size を渡すと、その大きさ（px）で上寄りの中ほどに出す。 */
export const floatUp = (key: string, scope: string, tab: Tab, size?: { w: number; h: number }): void => {
  const cur = floats.find((f) => same(f, key, scope, tab.id));
  if (cur) raise(cur);
  else if (size) floatOut(key, scope, tab, new DOMRect((innerWidth - size.w) / 2, innerHeight / 8, size.w, size.h));
  else floatOut(key, scope, tab, new DOMRect(innerWidth / 4, innerHeight / 8, innerWidth / 2, (innerHeight * 3) / 4));
};

/** ウィンドウを閉じ、元の枠に中身を戻す。 */
export const dock = (key: string, scope: string, tabId: string): void => setFloats(floats.filter((f) => !same(f, key, scope, tabId)));

const update = (f: Float, patch: Partial<Float>): void => setFloats(floats.map((x) => (same(x, f.key, f.scope, f.tab.id) ? { ...x, ...patch } : x)));

/** 開いている間に設定が変わっていても、今のウィンドウを返す。閉じていれば undefined。 */
export const latestFloat = (f: Float): Float | undefined => floats.find((x) => same(x, f.key, f.scope, f.tab.id));

/** ウィンドウの設定を変える。エクスプローラーの開いているフォルダーなど、ウィンドウごとの状態を保存するのに使う。 */
export const setFloatSettings = (f: Float, settings: WidgetSettings): void => update(f, { tab: { ...f.tab, settings } });

/** 押したウィンドウを、最小化していれば元に戻し、画面の外にあれば中に戻し、ほかのウィンドウより手前に出す。重なりの順は、一覧の後ろほど手前にする。 */
const raise = (f: Float): void => {
  const next = { ...inView(f), min: undefined };
  if (floats.at(-1) !== f || f.min || next.x !== f.x || next.y !== f.y) setFloats([...floats.filter((x) => x !== f), next]);
};

/** 最小化していないウィンドウのうち、いちばん手前のものか。 */
const isActive = (f: Float): boolean => floats.findLast((x) => !x.min) === f;

/** 最大化する。戻すときのために、ウィンドウだったことを覚えておく。 */
const maximize = (f: Float): void => update(f, { max: 'float' });

/** 最大化をやめ、最大化する前の状態に戻す。元の枠から最大化していたら、枠に戻す。 */
const unmaximize = (f: Float): void => (f.max === 'dock' ? dock(f.key, f.scope, f.tab.id) : update(f, { max: undefined }));

/** Windows のタスクバーと同じく、手前のウィンドウなら最小化し、そうでなければ元に戻して手前に出す。 */
const toggleTask = (f: Float): void => (isActive(f) ? update(f, { min: true }) : raise(f));

/**
 * タイトルバー右端のボタン。kind で見た目を決める。refresh は取り直し、archive はアーカイブ、close は外す、restore はウィンドウ化、max は最大化、min は最小化。
 * pressed を渡すと、切り替えのボタンとして押した状態を示す。
 */
export type Control = { kind: 'refresh' | 'archive' | 'close' | 'restore' | 'max' | 'min'; label: string; pressed?: boolean; onClick: (e: MouseEvent) => void };

/**
 * タイトルバー右端のボタンを、決まった枠に並べる。ボタンの大きさと間隔は CSS の --ctl と --ctl-gap で決める。
 * 見出しはボタンに重ならないよう、ウィジェットの style に controlsVar で渡したボタンの数の分だけ右を空ける。
 */
export const WindowControls = ({ controls }: { controls: Control[] }) =>
  controls.length > 0 && (
    <div class="window-controls">
      {controls.map((c) => (
        <button key={c.kind} class={`window-ctl is-${c.kind}`} aria-label={c.label} title={c.label} aria-pressed={c.pressed} onClick={c.onClick} />
      ))}
    </div>
  );

export const controlsVar = (controls: Control[]): string => `--controls: ${controls.length}`;

/**
 * タイトルバー、タブ、タスクバーのボタンの左端に CSS で描くアイコン。XP のスキンでだけ描く。
 * icon は絵文字か、/ で始まる画像の URL。画像は文字を空にして背景に描く。
 */
export const iconVar = (icon: string): string =>
  icon.startsWith('/') ? `--icon: ""; --icon-img: url(${JSON.stringify(icon)})` : `--icon: ${JSON.stringify(icon)}; --icon-img: none`;

// GitHub と JIRA のものは、サイトの favicon を使う。jira はサーバーが JIRA_BASE_URL に読み替える。
export const GITHUB_ICON = `/api/favicon?${new URLSearchParams({ site: 'https://github.com' })}`;
export const JIRA_ICON = '/api/favicon?site=jira';
export const NOTION_ICON = `/api/favicon?${new URLSearchParams({ site: 'https://www.notion.so' })}`;

/**
 * 元の枠に並べる、ウィンドウ化と最大化のボタン。onWindow は max を渡すと、最大化して浮かべる。
 * 浮かべている間の枠には、枠に戻すボタンだけを置く。
 */
export const dockedControls = (floating: boolean, onWindow: (e: MouseEvent, max?: boolean) => void): Control[] =>
  floating
    ? [{ kind: 'restore', label: '枠に固定', onClick: onWindow }]
    : [{ kind: 'restore', label: 'ウィンドウ化', onClick: onWindow }, { kind: 'max', label: '最大化', onClick: (e) => onWindow(e, true) }];

// 大きさを変えられる辺と角。n, s, e, w の組み合わせで、角は 2 文字にする。
export const EDGES = ['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw'];
const MIN = { w: MIN_W, h: MIN_H };

/** つかんだときのポインタの位置と、ウィンドウ。edge が無ければタイトルバーをつかんで動かす。 */
type DragStart = { x: number; y: number; f: Float; edge?: string };

/**
 * タイトルバーのドラッグで動かし、上下左右の辺と 4 つの角のドラッグで大きさを変える。
 * タスクバーには、ヘッダーの .taskbar-windows へ、このウィンドウのボタンを置く。
 * ウィンドウ化のボタンは、最大化していれば前の状態に戻し、ウィンドウなら onDock で元の枠に戻す。元の枠が無いウィンドウでは、最大化している間だけ出す。
 * 最大化のボタンは、最大化している間は出さない。
 * 最大化している間は、動かすことも大きさを変えることもできない。
 * 最大化している間は、押しても手前に出さない。上に重ねたほかのウィンドウを、後ろに隠さないためである。
 */
export const FloatFrame = ({ f, title, icon, controls, onDock, children }: { f: Float; title: string; icon: string; controls: Control[]; onDock?: () => void; children: ComponentChildren }) => {
  const drag = useRef<DragStart>();
  const [taskbar, setTaskbar] = useState<Element | null>(null);
  useEffect(() => setTaskbar(document.querySelector('.taskbar-windows')), []);
  // 重なりの順と、いちばん手前のウィンドウかは、ほかのウィンドウを手前に出したり最小化したりしても変わるので、一覧から選んで描き直す。
  const z = useFloats((list) => list.indexOf(f));
  const active = useFloats((list) => list.findLast((x) => !x.min) === f);
  const restore: Control[] = f.max
    ? [{ kind: 'restore', label: f.max === 'dock' ? '枠に戻す' : '元の大きさに戻す', onClick: () => unmaximize(f) }]
    : onDock ? [{ kind: 'restore', label: '枠に固定', onClick: onDock }] : [];
  const max: Control[] = f.max ? [] : [{ kind: 'max', label: '最大化', onClick: () => maximize(f) }];
  // 閉じるボタンは、Windows と同じく右端に置く。
  const all: Control[] = [
    ...controls.filter((c) => c.kind !== 'close'),
    { kind: 'min', label: '最小化', onClick: () => update(f, { min: true }) },
    ...restore,
    ...max,
    ...controls.filter((c) => c.kind === 'close'),
  ];
  const grab = (edge?: string) => ({
    onPointerDown: (e: PointerEvent) => {
      if (e.button !== 0 || f.max || (e.target as HTMLElement).closest('button, a, input, textarea, select')) return;
      // 中身の文字を選び始めると、ブラウザがドラッグを pointercancel で打ち切るので止める。
      e.preventDefault();
      drag.current = { x: e.clientX, y: e.clientY, f, edge };
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    },
    onPointerMove: (e: PointerEvent) => {
      const d = drag.current;
      if (!d) return;
      const dx = e.clientX - d.x;
      const dy = e.clientY - d.y;
      update(f, d.edge ? resizeRect(d.f, d.edge, dx, dy, MIN) : {
        x: clamp(d.f.x + dx, KEEP - f.w, innerWidth - KEEP),
        y: clamp(d.f.y + dy, 0, innerHeight - KEEP),
      });
    },
    onLostPointerCapture: () => {
      drag.current = undefined;
    },
  });
  return (
    <section
      class={`widget float-window${f.min ? ' is-min' : ''}${f.max ? ' is-max' : ''}`}
      aria-label={title}
      style={`${f.max ? '' : `left: ${f.x}px; top: ${f.y}px; width: ${f.w}px; height: ${f.h}px; `}--z: ${z}; ${controlsVar(all)}; ${iconVar(icon)}`}
      onPointerDown={() => !f.max && raise(f)}
    >
      <div class="section-head float-head" {...grab()} onDblClick={() => (f.max ? unmaximize(f) : maximize(f))}>
        <h2 class="section-title">{title}</h2>
      </div>
      <div class="widget-tab-panel">{children}</div>
      <WindowControls controls={all} />
      {!f.max && EDGES.map((edge) => <div key={edge} class={`float-edge is-${edge}`} {...grab(edge)} />)}
      {taskbar && createPortal(
        <button class="taskbar-window" title={title} style={iconVar(icon)} aria-pressed={active} onClick={() => toggleTask(f)}>{title}</button>,
        taskbar,
      )}
    </section>
  );
};

type WindowProps<P> = { f: Float; widgets: Record<string, Widget<P>>; data: P; titleOf?: (title: string) => string };

/** 浮かべたウィジェットを描く。titleOf で、どのセッションのものかなどを見出しに添える。 */
export const FloatWindow = <P extends object>({ f, widgets, data, titleOf = (t) => t }: WindowProps<P>) => {
  const w = widgets[kindOf(f.tab.id)];
  if (!w) return null;
  const onSettings = (settings: WidgetSettings) => update(f, { tab: { ...f.tab, settings } });
  return (
    <FloatFrame f={f} title={titleOf(f.tab.settings?.title || w.title)} icon={w.icon} controls={w.controls?.(data) ?? []} onDock={() => dock(f.key, f.scope, f.tab.id)}>
      <w.Body {...data} settings={f.tab.settings ?? {}} onSettings={onSettings} />
    </FloatFrame>
  );
};
