// 通知領域に常駐する、Claude のログインのアイコン。ダッシュボードから前回ログインしてからの時間を、人間が決めたタイマーに照らして見せ、ログインが切れる前にログインし直せるようにする。
import { useContext, useState } from 'preact/hooks';
import type { ClaudeLogin } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { dateTime, fmtSpan } from './format.ts';
import { NowContext } from './hooks.ts';

const LOGIN_ID = 'claude-login-flyout';
const STORAGE_KEY = 'claude-login-hours';
const HOUR = 60 * 60_000;
// これまでのログインは 8 日ほどで切れた。人間がタイマーを決めるまでは、少し手前の 7 日で切れるようにする。
const DEFAULT_HOURS = 7 * 24;
// claude-new.svg と同じ Claude の印を、リングの内側に収まる大きさで描く。
const SPARK = 'M10 10L15.5 10M10 10L14.2 12.3M10 10L12.9 14.7M10 10L10 14.8M10 10L7.4 14.9M10 10L5.9 12.4M10 10L4.5 10M10 10L5.8 7.8M10 10L7.1 5.4M10 10L10 5.3M10 10L12.6 5.2M10 10L14.1 7.6';

const loadHours = (): number => {
  try {
    return Number(localStorage.getItem(STORAGE_KEY)) || DEFAULT_HOURS;
  } catch {
    return DEFAULT_HOURS;
  }
};

/**
 * アイコンを囲むリングの長さでタイマーの残りを見せ、切れたらリングを赤くする。ダッシュボードからまだログインしていなければ、数え始める時刻が無いのでリングを出さない。
 * 押すと、前回ログインした時刻とタイマーの設定と、ログインし直すボタンを開く。タイマーはブラウザに保存する。
 */
export const LoginTray = ({ login }: { login?: ClaudeLogin }) => {
  const now = useContext(NowContext);
  const [hours, setHours] = useState(loadHours);
  const at = login?.at;
  const rest = at === undefined ? undefined : at + hours * HOUR - now;
  const expired = rest !== undefined && rest <= 0;
  const summary = rest === undefined ? 'Claude のログイン' : `Claude のログイン: ${expired ? 'タイマーが切れました' : `タイマーの残り ${fmtSpan(rest)}`}`;
  const save = (value: number) => {
    if (!(value > 0)) return;
    setHours(value);
    try {
      localStorage.setItem(STORAGE_KEY, String(value));
    } catch {
      // 保存できなくても、開いている間は決めたタイマーで数える。
    }
  };
  return (
    <>
      <button class={`login-tray${expired ? ' is-expired' : ''}`} popovertarget={LOGIN_ID} title={summary} aria-label={summary}>
        <svg viewBox="0 0 20 20" aria-hidden="true">
          <circle class="login-ring-track" cx="10" cy="10" r="8.5" />
          {rest !== undefined && (
            <circle class="login-ring" cx="10" cy="10" r="8.5" pathLength={100} transform="rotate(-90 10 10)" stroke-dasharray={`${expired ? 100 : Math.min(100, (rest / (hours * HOUR)) * 100)} 100`} />
          )}
          <path class="login-spark" d={SPARK} />
        </svg>
      </button>
      <div id={LOGIN_ID} class="login-flyout" popover>
        <b class="login-head">Claude のログイン</b>
        <p>{at === undefined ? 'ダッシュボードからは、まだログインしていません。' : `前回のログイン: ${dateTime(at)}（${fmtSpan(now - at)}前）`}</p>
        {rest !== undefined && <p class={expired ? 'login-alert' : undefined}>{expired ? 'タイマーが切れました。ログインが切れる前に、ログインし直してください。' : `タイマーの残り: ${fmtSpan(rest)}`}</p>}
        <label class="login-hours">
          タイマー
          <input class="input input-sm" type="number" min={1} step={1} value={hours} onChange={(e) => save(Number(e.currentTarget.value))} />
          時間
        </label>
        {login?.error && !login.running && <p class="login-alert">前回はログインし直せませんでした（{login.error}）</p>}
        <button class="btn btn-sm btn-primary" disabled={login?.running} onClick={() => void attempt('Claude のログイン', () => api.claudeLogin())}>
          {login?.running ? 'ブラウザでログインしています…' : 'ログインし直す'}
        </button>
      </div>
    </>
  );
};
