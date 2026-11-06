import { useEffect, useRef, useState } from "preact/hooks";
import { spanOf, type Span } from "./model.ts";
import { indexOf, wordKeyHandler } from "./wordKeys.ts";

interface Drag {
  anchor: number;
  moved: boolean;
}

export interface DragSelect {
  span: Span | null;
  /** キーボードで今いる単語の番号。この単語だけが Tab で止まる（roving tabindex）。 */
  cursor: number;
  /** ボタンを押したままか。操作欄はドラッグが終わってから出し、押している間に文の位置が動かないようにする。 */
  dragging: boolean;
  clear: () => void;
  handlers: {
    onMouseDown: (e: MouseEvent) => void;
    onMouseOver: (e: MouseEvent) => void;
    onKeyDown: (e: KeyboardEvent) => void;
  };
}

/**
 * 単語のドラッグで範囲を選ぶ。動かさずに離したときは 1 語のクリックとして `onClick` を呼ぶ。
 * Shift を押したクリックは、いまの範囲の先頭から伸ばす。
 * キーボードは左右の矢印で単語を移り、Shift+矢印で範囲を伸ばし、Enter か Space で 1 語を選ぶ。Escape で選択を外す。
 */
export const useDragSelect = (onClick: (index: number) => void, count: number): DragSelect => {
  const [span, setSpan] = useState<Span | null>(null);
  const [cursor, setCursor] = useState(0);
  const anchor = useRef(0);
  const [dragging, setDragging] = useState(false);
  const drag = useRef<Drag | null>(null);
  const click = useRef(onClick);
  click.current = onClick;

  useEffect(() => {
    const release = () => {
      const d = drag.current;
      drag.current = null;
      setDragging(false);
      if (d && !d.moved) click.current(d.anchor);
    };
    window.addEventListener("mouseup", release);
    return () => window.removeEventListener("mouseup", release);
  }, []);

  const onMouseDown = (e: MouseEvent) => {
    const i = indexOf(e);
    if (e.button !== 0 || i === null) return;
    const from = e.shiftKey && span ? span.from : i;
    drag.current = { anchor: from, moved: from !== i };
    anchor.current = from;
    setCursor(i);
    setDragging(true);
    setSpan(spanOf(from, i));
  };
  const onMouseOver = (e: MouseEvent) => {
    const d = drag.current;
    const i = indexOf(e);
    if (!d || i === null) return;
    if (i !== d.anchor) d.moved = true;
    setSpan(spanOf(d.anchor, i));
  };
  const onKeyDown = wordKeyHandler({ count, anchor, span, setSpan, setCursor, onSelect: (i) => click.current(i) });
  return {
    span,
    cursor,
    dragging,
    clear: () => setSpan(null),
    handlers: { onMouseDown, onMouseOver, onKeyDown },
  };
};
