// アプリ「FL Connect」のウィンドウ。開いている間だけ、ダッシュボードのサーバーが FL Studio とつながり、つないだセッションのエージェントが FL を読み書きできる。
// サーバーとは SSE でつなぎ、FL の状態とログを受け取る。ダッシュボードが起動し直して切れたら、つなぎ直す。
import { useEffect, useRef, useState } from 'preact/hooks';
import { describeAgent, describeChange } from '../shared/flText.ts';
import type { FlAgentEntry, FlConnectView, FlLogEntry, Snapshot } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { DesktopIcon } from './apps.tsx';
import { openFileFloat } from './Docs.tsx';
import { type FlAction, type FlStep, nextSteps } from './flSteps.ts';
import { type Float, FloatFrame, dock, floatUp, setFloatSettings } from './Float.tsx';
import { toast } from './toast.tsx';
import { Chip, Empty } from './ui.tsx';

/** ウィンドウの保存キーで、ダッシュボードに置くアプリの種類の名前でもある。FL とつなぐ道は 1 組なので、ウィンドウは 1 つだけ開く。 */
export const FL_KEY = 'flconnect';
export const FL_TITLE = 'FL Connect';
export const FL_ICON = '/flconnect.svg';
const SIZE = { w: 660, h: 640 };
/** 接続が切れてから、つなぎ直すまでの時間。 */
const RECONNECT_MS = 2000;

/** セッションの画面で開いたら、そのセッションにつなぐ。ホームから開いたときは、ウィンドウでセッションを選ぶまで、どのセッションにもつながない。 */
export const openFlConnect = (sessionId?: string): void => {
  floatUp(FL_KEY, '', { id: FL_KEY, settings: sessionId ? { session: sessionId } : undefined }, SIZE);
  // 開いていたウィンドウは手前に出るだけなので、つなぐセッションはサーバーで切り替える。
  if (sessionId) void attempt('FL Connect のセッションの切り替え', () => api.flSession(sessionId));
};

export const FlConnectIcon = ({ sessionId }: { sessionId?: string }) => (
  <DesktopIcon icon={FL_ICON} title={FL_TITLE} hint="FL Studio とつなぎ、エージェントが FL の操作を知り、FL を操作できるようにします。開いている間だけつながります。ダブルクリックで開きます" onLaunch={() => openFlConnect(sessionId)} />
);

const clock = (at: number): string => new Date(at).toTimeString().slice(0, 8);

/**
 * サーバーに SSE でつなぎ、届いた状態を返す。切れたら RECONNECT_MS 後につなぎ直す。
 * つなぎ直すときは、そのときのセッションを渡す。ダッシュボードが起動し直しても、同じセッションにつなぎ直すため。
 */
const useFlView = (session: () => string | undefined): FlConnectView | undefined => {
  const [view, setView] = useState<FlConnectView>();
  useEffect(() => {
    let source: EventSource | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let live = true;
    const connect = (): void => {
      source = new EventSource(api.flEventsUrl(session()));
      source.addEventListener('view', (e) => setView(JSON.parse((e as MessageEvent<string>).data) as FlConnectView));
      source.onerror = () => {
        source?.close();
        if (live) timer = setTimeout(connect, RECONNECT_MS);
      };
    };
    connect();
    return () => {
      live = false;
      clearTimeout(timer);
      source?.close();
    };
  }, []);
  return view;
};

/** FL と Gopher MCP とのつながり。Gopher MCP は、つながっているときだけ出す。つながっていないときの理由は、次にすることに出す。 */
const Links = ({ v }: { v: FlConnectView }) => {
  const h = v.midi.hello;
  return (
    <div class="fl-links">
      {v.midi.status === 'connected' && h ? <Chip tone="ok" title={`FL Connect のスクリプト ${h.script}（中身 ${h.core ?? '?'}）、MIDI スクリプトの API ${h.api}、Python ${h.python}`}>FL Studio {h.fl}</Chip> : <Chip tone="muted">FL Studio とつながっていない</Chip>}
      {v.gopher.status === 'connected' && <Chip tone="ok">Gopher MCP（ツール {v.gopher.tools ?? '?'} 個）</Chip>}
      {v.gopher.status === 'busy' && <Chip tone="warn" title={v.gopher.message}>Gopher MCP（Gopher が使っている）</Chip>}
    </div>
  );
};

const ACTIONS: Record<FlAction, () => Promise<{ message: string }>> = { loopback: api.flLoopback, install: api.flInstall, launch: api.flLaunch };

/** 導入で次にすること。済んでいれば何も出さない。ボタンを押した結果は、次に押すまで下に出す。 */
const NextSteps = ({ v }: { v: FlConnectView }) => {
  const [message, setMessage] = useState<string>();
  const steps = nextSteps(v);
  const run = (step: FlStep) => {
    const action = step.action;
    if (!action) return;
    setMessage(undefined);
    void attempt(action.label, ACTIONS[action.kind]).then((r) => r && setMessage(r.message));
  };
  if (!steps.length && !message) return null;
  return (
    <div class="fl-steps">
      {steps.map((s) => (
        <div key={s.key} class={`fl-step is-${s.tone}`} role={s.tone === 'warn' ? 'alert' : undefined}>
          <span class="fl-step-icon" aria-hidden="true">{s.tone === 'warn' ? '⚠' : 'ℹ'}</span>
          <div class="fl-step-body">
            <strong>{s.title}</strong>
            {s.text && <p>{s.text}</p>}
          </div>
          {s.action && (
            <button class="btn btn-sm" disabled={!!s.action.disabled} title={s.action.disabled} onClick={() => run(s)}>
              {s.action.label}
            </button>
          )}
        </div>
      ))}
      {message && (
        <div class="fl-step-result">
          <pre class="fl-pre">{message}</pre>
          <button class="btn btn-sm" aria-label="結果を閉じる" onClick={() => setMessage(undefined)}>×</button>
        </div>
      )}
    </div>
  );
};

/** remove_channels で消えるチャンネルを、人間に確かめる。消したチャンネルは FL の元に戻す履歴に残らない。 */
const Removal = ({ removal, sessionName }: { removal: NonNullable<FlConnectView['removal']>; sessionName: string }) => (
  <div class="fl-removal" role="alertdialog" aria-label="チャンネルを消してよいかの確認">
    <strong>「{sessionName}」のエージェントが、次のチャンネルを消そうとしています。</strong>
    <ul>
      {removal.channels.map((c) => (
        <li key={c.position}>
          {c.position} 番目「{c.name}」{c.plugin && c.plugin !== c.name ? `（${c.plugin}）` : ''}
        </li>
      ))}
    </ul>
    <p>消したチャンネルは、FL の元に戻す履歴に残らないので、Ctrl+Z では戻りません。</p>
    <div class="fl-actions">
      <button class="btn btn-danger" onClick={() => void attempt('チャンネルを消す許可', () => api.flRemoval(removal.id, true))}>
        消してよい
      </button>
      <button class="btn" onClick={() => void attempt('チャンネルを消すのを断る', () => api.flRemoval(removal.id, false))}>
        やめる
      </button>
    </div>
  </div>
);

const WHO = { you: 'You', agent: 'Agent' } as const;

/** エージェントが FL で動かしたもの。コードを渡すツールはコードを、ほかは引数を見せる。 */
const ranOf = (e: FlAgentEntry): string => e.detail ?? (typeof e.args.code === 'string' ? e.args.code : JSON.stringify(e.args, null, 2));

/** ログの 1 件。記録は生の値なので、ここで文に訳す。人間の操作は、開くと生の値の JSON を見せる。エージェントの 1 件は、開くと動かしたものと出力と、変わった値を見せる。 */
const LogItem = ({ e }: { e: FlLogEntry }) => {
  if (e.who === 'you') {
    const { id: _id, at: _at, who: _who, ...raw } = e;
    return (
      <li class="fl-log-item is-you">
        <details>
          <summary>
            <time>{clock(e.at)}</time> <span class="fl-who">{WHO.you}</span> {describeChange(e)}
          </summary>
          <pre class="fl-pre">{JSON.stringify(raw)}</pre>
        </details>
      </li>
    );
  }
  return (
    <li class={`fl-log-item is-agent${e.ok ? '' : ' is-failed'}`}>
      <details>
        <summary>
          <time>{clock(e.at)}</time> <span class="fl-who">{WHO.agent}</span> {describeAgent(e)}
          {e.changes.length ? <span class="fl-log-count">{e.changes.length} 件の変更</span> : null}
        </summary>
        {e.changes.length ? (
          <ul class="fl-changes">
            {e.changes.map((c, k) => <li key={k}>{describeChange(c)}</li>)}
          </ul>
        ) : null}
        <pre class="fl-pre">{ranOf(e)}</pre>
        {e.output && <pre class="fl-pre">{e.output}</pre>}
      </details>
    </li>
  );
};

/** ログ。新しいものを上に並べ、You と Agent のチェックで絞る。絞り方はウィンドウの設定に残す。 */
const Log = ({ f, v }: { f: Float; v: FlConnectView }) => {
  const settings = f.tab.settings ?? {};
  const shown = { you: settings.you !== 'off', agent: settings.agent !== 'off' };
  const toggle = (who: FlLogEntry['who'], on: boolean) => setFloatSettings(f, { ...settings, [who]: on ? 'on' : 'off' });
  const entries = v.log.filter((e) => shown[e.who]).reverse();
  return (
    <>
      <div class="fl-log-bar">
        <span class="fl-log-title">ログ</span>
        {(['you', 'agent'] as const).map((who) => (
          <label key={who}>
            <input type="checkbox" checked={shown[who]} onChange={(e) => toggle(who, e.currentTarget.checked)} /> {WHO[who]}
          </label>
        ))}
        {v.logFile && v.sessionId && (
          <a class="fl-log-file" href="#" title={`${v.logFile}。セッションの資料にも足してあります`} onClick={(e) => {
            e.preventDefault();
            openFileFloat(v.sessionId ?? '', v.logFile ?? '');
          }}>
            ファイルで開く
          </a>
        )}
      </div>
      {entries.length ? (
        <ul class="fl-log">
          {entries.map((e) => <LogItem key={e.id} e={e} />)}
        </ul>
      ) : (
        <Empty>{shown.you || shown.agent ? 'FL で操作するか、エージェントが FL を操作すると、ここに並びます。' : 'You か Agent に印を付けると、ログが並びます。'}</Empty>
      )}
    </>
  );
};

export const FlConnectFloat = ({ f, snapshot }: { f: Float; snapshot: Snapshot }) => {
  // つなぎ直すときに、そのときのセッションを読めるよう、ref に持つ。
  const sessionRef = useRef(f.tab.settings?.session);
  const v = useFlView(() => sessionRef.current);
  const bound = v?.sessionId;
  // サーバーで切り替えたセッションをウィンドウの設定に残し、ダッシュボードが起動し直したときも同じセッションにつなぐ。
  // 最初の状態が届くまでは、サーバーがまだ設定のセッションにつないでいないので、設定を変えない。
  const received = !!v;
  useEffect(() => {
    if (!received) return;
    sessionRef.current = bound;
    if (bound === f.tab.settings?.session) return;
    const { session: _old, ...rest } = f.tab.settings ?? {};
    setFloatSettings(f, bound ? { ...rest, session: bound } : rest);
  }, [received, bound]);
  // 確かめてほしいことが来たら、ウィンドウを手前に出して知らせる。
  const removalId = v?.removal?.id;
  useEffect(() => {
    if (!removalId) return;
    floatUp(FL_KEY, '', f.tab, SIZE);
    toast('FL Connect: エージェントがチャンネルを消してよいかを尋ねています');
  }, [removalId]);
  const sessions = snapshot.sessions.filter((s) => !s.archived || s.id === bound);
  const nameOf = (id: string): string => snapshot.sessions.find((s) => s.id === id)?.name ?? id.slice(0, 8);
  const close = { kind: 'close' as const, label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) };
  return (
    <FloatFrame f={f} title={bound ? `${nameOf(bound)} - ${FL_TITLE}` : FL_TITLE} icon={FL_ICON} controls={[close]}>
      <div class="widget-body fl-connect">
        {!v ? (
          <Empty>ダッシュボードにつないでいます…</Empty>
        ) : (
          <>
            <label class="inline-field fl-session">
              つなぐセッション
              <select class="input input-sm input-auto" value={bound ?? ''} onChange={(e) => void attempt('FL Connect のセッションの切り替え', () => api.flSession(e.currentTarget.value || undefined))}>
                <option value="">（選んでいない。記録をどこにも届けない）</option>
                {sessions.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}
                  </option>
                ))}
              </select>
            </label>
            {v.removal && <Removal removal={v.removal} sessionName={nameOf(v.removal.sessionId)} />}
            <Links v={v} />
            <NextSteps v={v} />
            <label class="fl-toggle" title="入れている間、エージェントは FL を読むだけで、値を変えられません">
              <input type="checkbox" checked={v.readOnly} onChange={(e) => void attempt('書き込みの切り替え', () => api.flSettings({ readOnly: e.currentTarget.checked }))} /> 書き込みを止める
            </label>
            {v.gopher.status === 'connected' && <p class="fl-note fl-warn">デバッグのポートが開いている間は、この PC のどのプロセスからも FL を操作できます。使い終えたら、FL をふつうに起動し直してください。</p>}
            {v.focus && (
              <p class="fl-focus">
                {v.focus.playing ? '▶ 再生中' : '■ 停止'}　{v.focus.tempo ?? '?'} BPM　前面: {v.focus.window ?? v.focus.caption ?? '?'}　channel {v.focus.channel}、mixer {v.focus.mixer}、pattern {v.focus.pattern}　元に戻す履歴 {v.focus.undo}
              </p>
            )}
            <Log f={f} v={v} />
          </>
        )}
      </div>
    </FloatFrame>
  );
};
