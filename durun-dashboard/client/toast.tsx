import { useEffect, useState } from 'preact/hooks';

type ToastKind = 'ok' | 'error';
type ToastItem = { id: number; text: string; kind: ToastKind };

const OK_MS = 4000;
const ERROR_MS = 8000;

let items: ToastItem[] = [];
let seq = 0;
const listeners = new Set<(list: ToastItem[]) => void>();

const emit = () => listeners.forEach((fn) => fn(items));

export const toast = (text: string, kind: ToastKind = 'ok'): void => {
  const id = ++seq;
  items = [...items, { id, text, kind }];
  emit();
  setTimeout(() => {
    items = items.filter((t) => t.id !== id);
    emit();
  }, kind === 'error' ? ERROR_MS : OK_MS);
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
    <div class="toasts" role="status" aria-live="polite">
      {list.map((t) => (
        <div key={t.id} class={`toast toast-${t.kind}`}>{t.text}</div>
      ))}
    </div>
  );
};
