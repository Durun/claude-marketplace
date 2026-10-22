// スリープや通信の途切れで止まったセッションを、続けるか尋ねる。
// スキンによらず XP の情報のメッセージボックスに倣う。ほかの操作をふさがないよう、モーダルにせず、タイトルバーをつかんで動かせるウィンドウで出す。
import { useRef, useState } from 'preact/hooks';
import type { Snapshot } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { clamp } from './layout.ts';
import { href } from './router.ts';

const TITLE = '応答の再開';
// タイトルバーを画面の外へ出しきらず、つかんで戻せるよう残す幅（px）。
const KEEP = 80;

/** タイトルバーをつかんだ位置の、ダイアログの左上からのずれと、ダイアログの幅（px）。 */
type Grab = { dx: number; dy: number; w: number };

/**
 * 自動継続モードでなければ、止まったセッションを並べて出す。OK を押すと並べたセッションをすべて続けさせ、× を押すと続けずに閉じる。
 * どちらを押しても、同じ停止ではもう出さない。
 */
export const OfflineDialog = ({ snapshot }: { snapshot: Snapshot | null }) => {
  const box = useRef<HTMLDialogElement>(null);
  const grab = useRef<Grab>();
  const [pos, setPos] = useState<{ x: number; y: number }>();
  const stopped = snapshot?.autoContinue ? [] : (snapshot?.sessions.filter((s) => s.offline) ?? []);
  if (!stopped.length) return null;
  const answer = (resume: boolean) => {
    for (const s of stopped) void attempt(`「${s.name}」の${resume ? '再開' : '確認'}`, () => api.answerOffline(s.id, resume));
  };
  return (
    <dialog ref={box} open class="xp-confirm offline-dialog" aria-label={TITLE} style={pos && `left: ${pos.x}px; top: ${pos.y}px; margin: 0`}>
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
        {TITLE}
      </h3>
      <button class="xp-confirm-close" aria-label="閉じる" title="続けずに閉じる" onClick={() => answer(false)} />
      <p class="xp-confirm-text">
        <i class="xp-info-icon" aria-hidden="true" />
        <span>スリープか通信の途切れで、応答が止まりました。<br />OK を押すと、次のセッションを続けます。</span>
      </p>
      <ul class="offline-dialog-list">
        {stopped.map((s) => <li key={s.id}><a href={href.session(s.id)}>{s.name}</a></li>)}
      </ul>
      <div class="xp-confirm-actions">
        <button onClick={() => answer(true)}>OK</button>
      </div>
    </dialog>
  );
};
