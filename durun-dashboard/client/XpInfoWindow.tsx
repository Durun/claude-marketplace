// スキンによらず XP の情報のメッセージボックスに倣うウィンドウ。止まったセッションの再開や、Claude が求めた認証を尋ねる。
// ほかの操作をふさがないよう、モーダルにせず、タイトルバーをつかんで動かせるウィンドウで出す。
import type { ComponentChildren } from 'preact';
import { useRef, useState } from 'preact/hooks';
import { clamp } from './layout.ts';

// タイトルバーを画面の外へ出しきらず、つかんで戻せるよう残す幅（px）。
const KEEP = 80;

/** タイトルバーをつかんだ位置の、ウィンドウの左上からのずれと、ウィンドウの幅（px）。 */
type Grab = { dx: number; dy: number; w: number };

/** × を押すと onClose を呼ぶ。closeTitle は × に添える説明。 */
export const XpInfoWindow = ({ title, closeTitle, onClose, children }: { title: string; closeTitle: string; onClose: () => void; children: ComponentChildren }) => {
  const box = useRef<HTMLDialogElement>(null);
  const grab = useRef<Grab>();
  const [pos, setPos] = useState<{ x: number; y: number }>();
  return (
    <dialog ref={box} open class="xp-confirm xp-info-window" aria-label={title} style={pos && `left: ${pos.x}px; top: ${pos.y}px; margin: 0`}>
      <h3
        class="xp-confirm-title"
        onPointerDown={(e) => {
          const r = box.current?.getBoundingClientRect();
          if (e.button !== 0 || !r) return;
          // 文字を選び始めると、ブラウザがドラッグを pointercancel で打ち切るので止める。
          e.preventDefault();
          grab.current = { dx: e.clientX - r.x, dy: e.clientY - r.y, w: r.width };
          e.currentTarget.setPointerCapture(e.pointerId);
        }}
        onPointerMove={(e) => {
          const g = grab.current;
          if (g) setPos({ x: clamp(e.clientX - g.dx, KEEP - g.w, innerWidth - KEEP), y: clamp(e.clientY - g.dy, 0, innerHeight - KEEP) });
        }}
        onLostPointerCapture={() => {
          grab.current = undefined;
        }}
      >
        {title}
      </h3>
      <button class="xp-confirm-close" aria-label="閉じる" title={closeTitle} onClick={onClose} />
      {children}
    </dialog>
  );
};
