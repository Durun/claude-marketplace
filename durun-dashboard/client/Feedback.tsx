import { useEffect, useRef, useState } from 'preact/hooks';
import type { FeedbackContext, FeedbackTarget, ImproveState, Snapshot } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { ImproveOptions, ImproveStatus } from './Improve.tsx';
import { toast } from './toast.tsx';
import { onSubmitKey } from './ui.tsx';

type Rect = FeedbackTarget['rect'];
type RootRef = { current: HTMLElement | null };

const TEXT_LIMIT = 500;
const HTML_LIMIT = 2000;
const LABEL_TEXT_LIMIT = 30;
// 要素を選ぶクリックがリンクの遷移やボタンの操作として扱われないよう、押下から click までを止める。
const BLOCKED_EVENTS = ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click'] as const;

const selectorOf = (el: Element): string => {
  const parts: string[] = [];
  for (let node: Element | null = el; node && node !== document.body; node = node.parentElement) {
    const current = node;
    const same = [...(current.parentElement?.children ?? [])].filter((x) => x.tagName === current.tagName);
    const nth = same.length > 1 ? `:nth-of-type(${same.indexOf(current) + 1})` : '';
    const id = current.id ? `#${CSS.escape(current.id)}` : '';
    const classes = [...current.classList].map((c) => `.${CSS.escape(c)}`).join('');
    parts.unshift(`${current.tagName.toLowerCase()}${id}${classes}${nth}`);
  }
  return parts.join(' > ') || 'body';
};

const rectOf = (el: Element): Rect => {
  const r = el.getBoundingClientRect();
  return { x: Math.round(r.x), y: Math.round(r.y), width: Math.round(r.width), height: Math.round(r.height) };
};

// メーターや印のように文字を持たない要素でも何の一部かが分かるよう、文字を持つ最も近い祖先までさかのぼる。
const textOf = (el: Element): string => {
  for (let node: Element | null = el; node; node = node.parentElement) {
    const text = (node instanceof HTMLElement ? node.innerText : (node.textContent ?? '')).trim();
    if (text) return text.slice(0, TEXT_LIMIT);
  }
  return '';
};

const toTarget = (el: Element): FeedbackTarget => ({
  selector: selectorOf(el),
  text: textOf(el),
  html: el.outerHTML.slice(0, HTML_LIMIT),
  rect: rectOf(el),
  route: location.hash,
});

const labelOf = (t: FeedbackTarget): string => {
  const last = (t.selector.split(' > ').pop() ?? '').replace(/:nth-of-type\(\d+\)$/, '');
  const text = t.text.replace(/\s+/g, ' ').slice(0, LABEL_TEXT_LIMIT);
  return text ? `${last}「${text}」` : last;
};

// カード全体をリンクにする ::after のような重ね合わせは、カーソルの位置に自分の箱がない。
// その下にある、カーソルを実際に含む要素を選ぶ。
const elementAt = (x: number, y: number): Element | undefined =>
  document.elementsFromPoint(x, y).find((el) => {
    const r = el.getBoundingClientRect();
    return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
  });

const contextOf = (snapshot: Snapshot | null): FeedbackContext => ({
  url: location.href,
  viewport: { width: innerWidth, height: innerHeight, devicePixelRatio },
  colorScheme: matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light',
  userAgent: navigator.userAgent,
  reducedMotion: matchMedia('(prefers-reduced-motion: reduce)').matches,
  snapshot,
});

/** 有効な間はパネルの外へのクリックを横取りし、カーソルの下の要素の位置を返す。 */
const usePicker = (active: boolean, root: RootRef, onPick: (el: Element) => void, onCancel: () => void): Rect | undefined => {
  const [rect, setRect] = useState<Rect>();
  useEffect(() => {
    if (!active) return undefined;
    let x = -1;
    let y = -1;
    // 通知領域の改善要望のボタンは要素として選ばず、押してフォームを閉じられるようにする。
    const inWidget = (e: Event): boolean => e.target instanceof Element && (!!root.current?.contains(e.target) || !!e.target.closest('.fb-tray'));
    const update = () => {
      const el = elementAt(x, y);
      setRect(el ? rectOf(el) : undefined);
    };
    const move = (e: PointerEvent) => {
      x = e.clientX;
      y = e.clientY;
      if (inWidget(e)) setRect(undefined);
      else update();
    };
    const block = (e: MouseEvent) => {
      if (inWidget(e)) return;
      e.preventDefault();
      e.stopPropagation();
      const el = e.type === 'click' ? elementAt(e.clientX, e.clientY) : undefined;
      if (el) onPick(el);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !e.isComposing) onCancel();
    };
    const listening = new AbortController();
    const options = { capture: true, signal: listening.signal };
    window.addEventListener('pointermove', move, options);
    window.addEventListener('scroll', update, options);
    window.addEventListener('keydown', key, options);
    for (const type of BLOCKED_EVENTS) window.addEventListener(type, block, options);
    document.documentElement.classList.add('fb-picking');
    return () => {
      listening.abort();
      document.documentElement.classList.remove('fb-picking');
      setRect(undefined);
    };
  }, [active]);
  return rect;
};

const TargetList = ({ targets, onRemove }: { targets: FeedbackTarget[]; onRemove: (t: FeedbackTarget) => void }) => (
  <ul class="fb-targets" aria-label="指している要素">
    {targets.map((t) => (
      <li key={t.selector} class="fb-target">
        <span class="fb-target-label mono" title={t.selector}>{labelOf(t)}</span>
        <button class="fb-icon" aria-label="添付を外す" onClick={() => onRemove(t)}>×</button>
      </li>
    ))}
  </ul>
);

type PanelProps = {
  improve?: ImproveState;
  text: string;
  targets: FeedbackTarget[];
  busy: boolean;
  onText: (text: string) => void;
  onRemove: (t: FeedbackTarget) => void;
  onSend: () => void;
  onClose: () => void;
};

const FeedbackPanel = (p: PanelProps) => {
  const area = useRef<HTMLTextAreaElement>(null);
  const [options, setOptions] = useState(false);
  // 開いたらすぐ書けるようにする。要素を選ぶクリックは押下を止めているので、フォーカスは textarea に残る。
  useEffect(() => area.current?.focus(), []);
  const empty = !p.text.trim() && p.targets.length === 0;
  return (
    <section class="card fb-panel" aria-label="改善要望">
      <header class="fb-head">
        <h2 class="fb-title">改善要望</h2>
        {p.improve && (
          <button class="fb-icon" aria-label="自動実装の設定" aria-expanded={options} onClick={() => setOptions((v) => !v)}>⚙︎</button>
        )}
        <button class="fb-icon" aria-label="閉じる" onClick={p.onClose}>×</button>
      </header>
      {options && p.improve && <ImproveOptions improve={p.improve} />}
      <textarea
        ref={area}
        class="input"
        rows={4}
        placeholder="気づいたことや直してほしいこと（Cmd/Ctrl + Enter で送信）"
        value={p.text}
        onInput={(e) => p.onText(e.currentTarget.value)}
        onKeyDown={(e) => onSubmitKey(e, p.onSend)}
      />
      {p.targets.length > 0 && <TargetList targets={p.targets} onRemove={p.onRemove} />}
      <div class="actions">
        <span class="help">画面の要素をクリックすると要望に添えます。</span>
        <span class="spacer" />
        <button class="btn btn-primary" disabled={p.busy || empty} onClick={p.onSend}>送信</button>
      </div>
      <p class="help fb-help">画面の URL、表示中のデータ、ウィンドウの大きさ、OS を添えて ~/.durun-dashboard/feedback/ に保存します。</p>
      {p.improve && <ImproveStatus improve={p.improve} />}
    </section>
  );
};

type FeedbackProps = { snapshot: Snapshot | null; open: boolean; setOpen: (open: boolean) => void };

/** 画面の右下に常に置く、ダッシュボードへの改善要望の入口。止まっている間は電球のアイコンにする。XP のスキンでは、右下のボタンの代わりにタスクバーの通知領域から開く。 */
export const Feedback = ({ snapshot, open, setOpen }: FeedbackProps) => {
  const root = useRef<HTMLDivElement>(null);
  const [text, setText] = useState('');
  const [targets, setTargets] = useState<FeedbackTarget[]>([]);
  const [busy, setBusy] = useState(false);
  const improve = snapshot?.improve;
  const running = !!improve?.running;
  const pick = (el: Element) => {
    const target = toTarget(el);
    setTargets((list) => (list.some((t) => t.selector === target.selector) ? list : [...list, target]));
  };
  const rect = usePicker(open, root, pick, () => setOpen(false));
  const send = async () => {
    if (busy || (!text.trim() && targets.length === 0)) return;
    setBusy(true);
    const res = await attempt('要望の保存', () => api.feedback(text, targets, contextOf(snapshot)));
    setBusy(false);
    if (!res) return;
    setText('');
    setTargets([]);
    setOpen(false);
    toast(`要望を保存しました（${res.path}）`);
  };
  return (
    <div class="fb" ref={root}>
      {rect && <div class="fb-highlight" style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height }} />}
      {!open && (
        <button
          class={`fb-trigger${running ? ' is-running' : ''}`}
          title={running ? improve?.current : '改善要望'}
          aria-label={running ? undefined : '改善要望'}
          onClick={() => setOpen(true)}
        >
          {running ? (
            '改善中'
          ) : (
            <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true">
              <path d="M6 11.5v-1.2A4.5 4.5 0 1 1 10 10.3v1.2z" />
              <path d="M6.3 13.5h3.4M7 15h2" />
            </svg>
          )}
        </button>
      )}
      {open && (
        <FeedbackPanel
          improve={improve}
          text={text}
          targets={targets}
          busy={busy}
          onText={setText}
          onRemove={(t) => setTargets((list) => list.filter((x) => x !== t))}
          onSend={send}
          onClose={() => setOpen(false)}
        />
      )}
    </div>
  );
};
