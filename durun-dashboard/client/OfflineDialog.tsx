// スリープや通信の途切れで止まったセッションを、続けるか尋ねる。
import type { Snapshot } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { href } from './router.ts';
import { XpInfoWindow } from './XpInfoWindow.tsx';

/**
 * 自動継続モードでなければ、止まったセッションを並べて出す。OK を押すと並べたセッションをすべて続けさせ、× を押すと続けずに閉じる。
 * どちらを押しても、同じ停止ではもう出さない。
 */
export const OfflineDialog = ({ snapshot }: { snapshot: Snapshot | null }) => {
  const stopped = snapshot?.autoContinue ? [] : (snapshot?.sessions.filter((s) => s.offline) ?? []);
  if (!stopped.length) return null;
  const answer = (resume: boolean) => {
    for (const s of stopped) void attempt(`「${s.name}」の${resume ? '再開' : '確認'}`, () => api.answerOffline(s.id, resume));
  };
  return (
    <XpInfoWindow title="応答の再開" closeTitle="続けずに閉じる" onClose={() => answer(false)}>
      <p class="xp-confirm-text">
        <i class="xp-info-icon" aria-hidden="true" />
        <span>スリープか通信の途切れで、応答が止まりました。<br />OK を押すと、次のセッションを続けます。</span>
      </p>
      <ul class="xp-info-list">
        {stopped.map((s) => <li key={s.id}><a href={href.session(s.id)}>{s.name}</a></li>)}
      </ul>
      <div class="xp-confirm-actions">
        <button onClick={() => answer(true)}>OK</button>
      </div>
    </XpInfoWindow>
  );
};
