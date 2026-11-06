import { useEffect, useRef, useState } from "preact/hooks";
import { spanOf, type Span } from "./model.ts";

interface Drag {
  anchor: number;
  moved: boolean;
}

/** イベントの対象の単語の番号。単語の外なら `null`。 */
const indexOf = (e: Event): number | null => {
  const raw = (e.target as HTMLElement | null)?.closest("[data-i]")?.getAttribute("data-i");
  return raw === null || raw === undefined ? null : Number(raw);
};

export interface DragSelect {
  span: Span | null;
  /** ボタンを押したままか。操作欄はドラッグが終わってから出し、押している間に文の位置が動かないようにする。 */
  dragging: boolean;
  clear: () => void;
  handlers: { onMouseDown: (e: MouseEvent) => void; onMouseOver: (e: MouseEvent) => void };
}

/**
 * 単語のドラッグで範囲を選ぶ。動かさずに離したときは 1 語のクリックとして `onClick` を呼ぶ。
 * Shift を押したクリックは、いまの範囲の頭から伸ばす。
 */
export const useDragSelect = (onClick: (index: number) => void): DragSelect => {
  const [span, setSpan] = useState<Span | null>(null);
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
    const anchor = e.shiftKey && span ? span.from : i;
    drag.current = { anchor, moved: anchor !== i };
    setDragging(true);
    setSpan(spanOf(anchor, i));
  };
  const onMouseOver = (e: MouseEvent) => {
    const d = drag.current;
    const i = indexOf(e);
    if (!d || i === null) return;
    if (i !== d.anchor) d.moved = true;
    setSpan(spanOf(d.anchor, i));
  };
  return { span, dragging, clear: () => setSpan(null), handlers: { onMouseDown, onMouseOver } };
};
