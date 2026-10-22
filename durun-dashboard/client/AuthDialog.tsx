// Claude が求めた認証を、進めてよいか尋ねる。同じ相手の認証を求めたセッションは、1 つのウィンドウにまとめて並べる。
import type { Snapshot } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { href } from './router.ts';
import { XpInfoWindow } from './XpInfoWindow.tsx';

/** 尋ねている認証のうち、最も古いものを出す。答えると、次に古いものを出す。 */
export const AuthDialog = ({ snapshot }: { snapshot: Snapshot | null }) => {
  const request = snapshot?.auth?.[0];
  if (!snapshot || !request) return null;
  const answer = (ok: boolean) => void attempt('認証への答え', () => api.answerAuth(request.id, ok));
  const sessions = snapshot.sessions.filter((s) => request.sessionIds.includes(s.id));
  return (
    <XpInfoWindow key={request.id} title={request.title} closeTitle="認証せずに閉じる" onClose={() => answer(false)}>
      <p class="xp-confirm-text">
        <i class="xp-info-icon" aria-hidden="true" />
        <span>次のセッションが、{request.title}を求めています。<br />OK を押すと、<code>{request.command}</code> で認証します。</span>
      </p>
      <ul class="xp-info-list">
        {sessions.map((s) => <li key={s.id}><a href={href.session(s.id)}>{s.name}</a></li>)}
      </ul>
      <div class="xp-confirm-actions">
        <button onClick={() => answer(true)}>OK</button>
        <button onClick={() => answer(false)}>キャンセル</button>
      </div>
    </XpInfoWindow>
  );
};
