import type { MutableRef } from "preact/hooks";
import { spanOf, type Span } from "./model.ts";

/** イベントの対象の単語の番号。単語の外なら `null`。 */
export const indexOf = (e: Event): number | null => {
  const raw = (e.target as HTMLElement | null)?.closest("[data-i]")?.getAttribute("data-i");
  return raw === null || raw === undefined ? null : Number(raw);
};

const ARROW_STEP: Record<string, number | undefined> = { ArrowLeft: -1, ArrowRight: 1 };
const SELECT_KEYS: ReadonlySet<string> = new Set(["Enter", " "]);

export interface WordKeysState {
  count: number;
  anchor: MutableRef<number>;
  span: Span | null;
  setSpan: (span: Span | null) => void;
  setCursor: (index: number) => void;
  /** Enter か Space で 1 語を選んだとき。 */
  onSelect: (index: number) => void;
}

/**
 * 単語のキーボード操作。左右の矢印で単語を移り、Shift+矢印で範囲を伸ばし、Enter か Space で 1 語を選ぶ。Escape で選択を外す。
 * Space と矢印は再生のキーでもあるので、単語が処理したキーは外へ伝えない。
 */
export const wordKeyHandler = (s: WordKeysState) => (e: KeyboardEvent) => {
  const at = indexOf(e);
  const step = ARROW_STEP[e.key];
  const handled = step !== undefined || SELECT_KEYS.has(e.key) || e.key === "Escape";
  if (at === null || !handled) return;
  e.preventDefault();
  e.stopPropagation();
  if (e.key === "Escape") return s.setSpan(null);
  if (step === undefined) {
    s.anchor.current = at;
    s.setSpan(spanOf(at, at));
    return s.onSelect(at);
  }
  const next = Math.min(Math.max(at + step, 0), s.count - 1);
  // 範囲が無いところから Shift で始めたときは、いまの単語を起点にする。
  if (e.shiftKey && s.span === null) s.anchor.current = at;
  if (!e.shiftKey) s.anchor.current = next;
  s.setCursor(next);
  if (e.shiftKey) s.setSpan(spanOf(s.anchor.current, next));
  (e.currentTarget as HTMLElement).querySelector<HTMLElement>(`[data-i="${next}"]`)?.focus();
};
