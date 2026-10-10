// 送信欄の + で入る「なんでも添付」。画面上の URL・ファイルパス・資料・バッジをクリックすると、その送信欄に書き足す。
import { useEffect, useRef, useState } from 'preact/hooks';

// 書き足す値を data-attach に持つ要素と、外へのリンク。ダッシュボードの中の画面へのリンクは、Claude が読めないので拾わない。
const ATTACHABLE = '[data-attach], a[href^="http"]';

/** at の位置に piece を入れ、前の語とは空白で区切り、続けて書けるよう後ろにも空白を足す。入れた後のカーソル位置も返す。 */
export const insertAt = (value: string, at: number, piece: string): { value: string; caret: number } => {
  const before = value.slice(0, at);
  const head = `${before}${before && !/\s$/.test(before) ? ' ' : ''}${piece} `;
  return { value: head + value.slice(at), caret: head.length };
};

// 書き足す先は 1 つに限る。別の送信欄で入ったら、前の送信欄の添付モードを抜ける。
let stopOther: (() => void) | undefined;

/** 添付モードの間は、拾えるものへのクリックを横取りし、リンクを開かずに insert へ渡す。Esc で抜ける。 */
export const useAttachMode = (insert: (text: string) => void) => {
  const [on, setOn] = useState(false);
  const latest = useRef(insert);
  latest.current = insert;
  useEffect(() => {
    if (!on) return;
    const stop = () => setOn(false);
    stopOther?.();
    stopOther = stop;
    const click = (e: MouseEvent) => {
      const el = e.target instanceof Element ? e.target.closest(ATTACHABLE) : null;
      if (!el) return;
      e.preventDefault();
      e.stopPropagation();
      latest.current(el.getAttribute('data-attach') ?? (el as HTMLAnchorElement).href);
    };
    const key = (e: KeyboardEvent) => e.key === 'Escape' && stop();
    document.addEventListener('click', click, true);
    document.addEventListener('keydown', key);
    document.body.classList.add('is-attaching');
    return () => {
      if (stopOther === stop) stopOther = undefined;
      document.removeEventListener('click', click, true);
      document.removeEventListener('keydown', key);
      document.body.classList.remove('is-attaching');
    };
  }, [on]);
  return { on, toggle: () => setOn((v) => !v) };
};
