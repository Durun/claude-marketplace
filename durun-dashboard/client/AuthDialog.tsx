// Claude が求めた認証を、進めてよいか尋ねる。同じ相手の認証を求めたセッションは、1 つのウィンドウにまとめて並べる。
// ブラウザ認証は、「進む」でログイン画面を開き、人間がログインを終えて「認証した」を押したら、Claude に返す。
// Claude のログインには、ダッシュボードから前回ログインした時刻を添え、ログインが何日もったかを人間がタイマーに使えるようにする。
import { useState } from 'preact/hooks';
import type { Snapshot } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { dateTime, fmtSpan } from './format.ts';
import { href } from './router.ts';
import { XpInfoWindow } from './XpInfoWindow.tsx';

/** 尋ねている認証のうち、最も古いものを出す。答えると、次に古いものを出す。 */
export const AuthDialog = ({ snapshot }: { snapshot: Snapshot | null }) => {
  // ログイン画面を開いたブラウザ認証の id。ログインする前に「認証した」を押さないよう、開くまで押せなくする。
  const [opened, setOpened] = useState<string>();
  const request = snapshot?.auth?.[0];
  if (!snapshot || !request) return null;
  const answer = (ok: boolean) => void attempt('認証への答え', () => api.answerAuth(request.id, ok));
  const sessions = snapshot.sessions.filter((s) => request.sessionIds.includes(s.id));
  const { url, lastLoginAt } = request;
  const open = () => {
    window.open(url, '_blank', 'noopener');
    setOpened(request.id);
  };
  return (
    <XpInfoWindow key={request.id} title={request.title} closeTitle="認証せずに閉じる" onClose={() => answer(false)}>
      <p class="xp-confirm-text">
        <i class="xp-info-icon" aria-hidden="true" />
        <span>
          次のセッションが、{request.title}を求めています。<br />
          {lastLoginAt && <>前回のログインは {dateTime(lastLoginAt)}（{fmtSpan(snapshot.now - lastLoginAt)}前）です。<br /></>}
          {url ?<>「進む」を押すと、ブラウザで <code>{url}</code> を開きます。ログインを終えたら、「認証した」を押してください。</> : <>OK を押すと、<code>{request.command}</code> で認証します。</>}
        </span>
      </p>
      <ul class="xp-info-list">
        {sessions.map((s) => <li key={s.id}><a href={href.session(s.id)}>{s.name}</a></li>)}
      </ul>
      <div class="xp-confirm-actions">
        {url ? (
          <>
            <button onClick={open}>進む</button>
            <button disabled={opened !== request.id} onClick={() => answer(true)}>認証した</button>
          </>
        ) : (
          <button onClick={() => answer(true)}>OK</button>
        )}
        <button onClick={() => answer(false)}>キャンセル</button>
      </div>
    </XpInfoWindow>
  );
};
