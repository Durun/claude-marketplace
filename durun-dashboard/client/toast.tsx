import { useEffect, useState } from 'preact/hooks';

type ToastKind = 'ok' | 'error';
/** href を持つものは、押すとその画面へ移る。 */
type ToastItem = { id: number; text: string; kind: ToastKind; href?: string };

const OK_MS = 4000;
// 失敗と、押して移れるお知らせは、読んで動けるよう長く出す。
const LONG_MS = 8000;
// 消えるときに、薄くなっていく時間。
const FADE_MS = 600;

let items: ToastItem[] = [];
let seq = 0;
const listeners = new Set<(list: ToastItem[]) => void>();

const emit = () => listeners.forEach((fn) => fn(items));

const remove = (id: number): void => {
  items = items.filter((t) => t.id !== id);
  emit();
};

export const toast = (text: string, kind: ToastKind = 'ok', href?: string): void => {
  items = [...items, { id: ++seq, text, kind, href }];
  emit();
};

/**
 * 出してから決まった時間がたつと、薄くなって消える。マウスを載せている間は消えず、薄くなりかけていても元に戻す。
 * 離すと、時間を数え直す。
 */
const Toast = ({ t }: { t: ToastItem }) => {
  const [hover, setHover] = useState(false);
  const [leaving, setLeaving] = useState(false);
  useEffect(() => {
    if (hover) return;
    const timer = setTimeout(() => setLeaving(true), t.kind === 'error' || t.href ? LONG_MS : OK_MS);
    return () => clearTimeout(timer);
  }, [hover]);
  useEffect(() => {
    if (!leaving) return;
    const timer = setTimeout(() => remove(t.id), FADE_MS);
    return () => clearTimeout(timer);
  }, [leaving]);
  const props = {
    class: `toast toast-${t.kind}${leaving ? ' is-leaving' : ''}`,
    onPointerEnter: () => {
      setHover(true);
      setLeaving(false);
    },
    onPointerLeave: () => setHover(false),
  };
  return t.href ? <a {...props} href={t.href} onClick={() => remove(t.id)}>{t.text}</a> : <div {...props}>{t.text}</div>;
};

export const Toasts = () => {
  const [list, setList] = useState(items);
  useEffect(() => {
    listeners.add(setList);
    return () => {
      listeners.delete(setList);
    };
  }, []);
  return (
    <div class="toasts" role="status" aria-live="polite" style={`--fade: ${FADE_MS}ms`}>
      {list.map((t) => <Toast key={t.id} t={t} />)}
    </div>
  );
};
