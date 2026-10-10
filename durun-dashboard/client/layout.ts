/*
 * ダッシュボードのウィジェットの配置。COLS 列のグリッドに、左上の升目 (x, y) と、幅と高さの升目の数 (w, h) で枠を置く。
 * 升目は、ブラウザに見えている広さに対する割合で決める。列は幅を COLS 等分し、行はグリッドの上端から画面の下端までを ROWS 等分する。
 */
/** settings は、そのウィジェットだけの設定（JIRA の一覧の JQL など）。 */
export type WidgetSettings = Record<string, string>;
/** 枠に入れたウィジェット。id は種類の名前か「種類:追加した時刻」にし、同じ種類を複数置いても重ならないようにする。 */
export type Tab = { id: string; settings?: WidgetSettings };
/** グリッドに置く枠。tabs は枠に重ねたウィジェットで、2 つ以上あればタブで切り替える。active は選んでいるタブの id。 */
export type Placement = { id: string; x: number; y: number; w: number; h: number; tabs: Tab[]; active?: string };
export type Layout = Placement[];
export type Size = Pick<Placement, 'w' | 'h'>;
type Box = Pick<Placement, 'id' | 'x' | 'y' | 'w' | 'h'>;

export const COLS = 24;
export const ROWS = 24;
// 幅と高さは 1 升まで縮められる。収まらない見出しは枠で切り、本文はウィジェットの中でスクロールする。
const MIN_SPAN = 1;

export const kindOf = (id: string): string => id.replace(/:.*/, '');

export const clamp = (v: number, min: number, max: number): number => Math.min(Math.max(v, min), max);

/** 画面上の位置と大きさ（px）。 */
export type Rect = { x: number; y: number; w: number; h: number };

/**
 * edge の辺をつかんで dx, dy だけ動かした大きさ。edge は n, s, e, w の組み合わせで、角は 2 つの辺を同時に動かす。
 * 左と上の辺は、反対の辺を動かさないよう位置も動かす。上の辺は、タイトルバーをつかめるよう画面の上端で止める。
 */
export const resizeRect = (r: Rect, edge: string, dx: number, dy: number, min: { w: number; h: number }): Rect => {
  let { x, y, w, h } = r;
  if (edge.includes('e')) w = Math.max(min.w, r.w + dx);
  if (edge.includes('s')) h = Math.max(min.h, r.h + dy);
  if (edge.includes('w')) {
    w = Math.max(min.w, r.w - dx);
    x = r.x + r.w - w;
  }
  if (edge.includes('n')) {
    h = Math.max(min.h, Math.min(r.h - dy, r.y + r.h));
    y = r.y + r.h - h;
  }
  return { x, y, w, h };
};

const overlaps = (a: Box, b: Box): boolean =>
  a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

/** 上から、同じ高さなら左から並べる。狭い画面では、この順に縦に積む。 */
export const readingOrder = <B extends Box>(layout: B[]): B[] => layout.toSorted((a, b) => a.y - b.y || a.x - b.x);

const pushDown = <B extends Box>(p: B, placed: Box[]): B => {
  const hit = placed.find((q) => overlaps(q, p));
  return hit ? pushDown({ ...p, y: hit.y + hit.h }, placed) : p;
};

/** item を置き、重なるウィジェットはその下へ押し出す。押し出した先で重なるものも、上から順に押し出す。配列の順は保つ。 */
const place = <B extends Box>(layout: B[], item: B): B[] => {
  const placed: B[] = [item];
  for (const p of readingOrder(layout.filter((q) => q.id !== item.id))) placed.push(pushDown(p, placed));
  return layout.map((p) => placed.find((q) => q.id === p.id) ?? p);
};

/** dx 列、dy 行だけ動かす。左右はグリッドからはみ出させず、上は先頭の行で止める。 */
export const moveWidget = <B extends Box>(layout: B[], id: string, dx: number, dy: number): B[] => {
  const p = layout.find((q) => q.id === id);
  return p ? place(layout, { ...p, x: clamp(p.x + dx, 0, COLS - p.w), y: Math.max(0, p.y + dy) }) : layout;
};

/** edge の辺か角（resizeRect と同じ書き方）を dx 列、dy 行だけ動かして、大きさを変える。左右はグリッドからはみ出させない。 */
export const resizeWidget = <B extends Box>(layout: B[], id: string, dx: number, dy: number, edge = 'se'): B[] => {
  const p = layout.find((q) => q.id === id);
  if (!p) return layout;
  const r = resizeRect(p, edge, dx, dy, { w: MIN_SPAN, h: MIN_SPAN });
  const x = Math.max(0, r.x);
  return place(layout, { ...p, ...r, x, w: Math.min(r.x + r.w, COLS) - x });
};

/** item を足し、重なるウィジェットは下へ押し出す。 */
export const addWidget = <B extends Box>(layout: B[], item: B): B[] => place([...layout, item], item);

/** keep に合うタブだけを残す。ウィジェットが無くなった枠は消す。 */
export const keepTabs = (layout: Layout, keep: (t: Tab) => boolean): Layout =>
  layout.flatMap((p) => {
    const tabs = p.tabs.filter(keep);
    return tabs.length > 0 ? [{ ...p, tabs }] : [];
  });

export const placementAt = (layout: Layout, x: number, y: number): Placement | undefined =>
  layout.find((p) => x >= p.x && x < p.x + p.w && y >= p.y && y < p.y + p.h);

/** tab を (x, y) の升目に置くときに、タブとして重ねる先の枠。ほかの枠から移すときは、元の枠から外した後の配置で探す。 */
export const dropTarget = (layout: Layout, tab: Tab, x: number, y: number): Placement | undefined =>
  placementAt(keepTabs(layout, (t) => t.id !== tab.id), x, y);

/**
 * tab を (x, y) の升目に置く。そこに枠があれば、タブとして重ねて選ぶ。
 * 空いていれば、size の大きさで id の枠を新しく作り、重なる枠を下へ押し出す。ほかの枠から移すときは、元の枠から外す。
 */
export const dropTab = (layout: Layout, tab: Tab, x: number, y: number, size: Size, id: string): Layout => {
  const rest = keepTabs(layout, (t) => t.id !== tab.id);
  const target = placementAt(rest, x, y);
  if (target) return rest.map((p) => (p === target ? { ...p, tabs: [...p.tabs, tab], active: tab.id } : p));
  return addWidget(rest, { id, x, y, w: Math.min(size.w, COLS - x), h: size.h, tabs: [tab] });
};

/** from の枠のタブを、すべて to の枠にタブとして重ねる。重ねた後は、from で選んでいたタブを選ぶ。 */
export const mergeInto = (layout: Layout, from: string, to: string): Layout => {
  const src = layout.find((p) => p.id === from);
  if (!src || !layout.some((p) => p.id === to)) return layout;
  const active = src.tabs.find((t) => t.id === src.active)?.id ?? src.tabs[0]?.id;
  return layout.flatMap((p) => (p.id === from ? [] : p.id === to ? [{ ...p, tabs: [...p.tabs, ...src.tabs], active }] : [p]));
};

/** 前の形の配置。升目は今の半分の細かさで数え、タブは「タブ」のウィジェットの設定に、入れる種類と、中のウィジェットの設定（「s:種類」の JSON）を持つ。 */
type SavedBefore = Box & { settings?: WidgetSettings };

const parseSettings = (raw: string | undefined): WidgetSettings => {
  try {
    return raw ? (JSON.parse(raw) as WidgetSettings) : {};
  } catch {
    return {};
  }
};

const migrate = ({ id, x, y, w, h, settings = {} }: SavedBefore): Placement => {
  const box = { id, x: x * 2, y: y * 2, w: w * 2, h: h * 2 };
  if (kindOf(id) !== 'tabs') return { ...box, tabs: [{ id, settings }] };
  const kinds = (settings.tabs ?? '').split(',').filter(Boolean);
  const tabs = kinds.map((k) => ({ id: `${k}:${id}`, settings: parseSettings(settings[`s:${k}`]) }));
  return { ...box, tabs, active: settings.active && `${settings.active}:${id}` };
};

/** 保存していた配置を読む。前の形のものは、升目の数を 2 倍にし、「タブ」のウィジェットを、選んでいた種類を重ねた枠に直す。 */
export const fromSaved = (saved: (Placement | SavedBefore)[]): Layout => saved.map((p) => ('tabs' in p ? p : migrate(p)));
