// アプリ「FL Connect」のウィンドウ。開いている間だけ、ダッシュボードのサーバーが FL Studio とつながり、つないだセッションのエージェントが FL を読み書きできる。
// サーバーとは SSE でつなぎ、FL の状態と記録を受け取る。ダッシュボードが起動し直して切れたら、つなぎ直す。
import { useEffect, useRef, useState } from 'preact/hooks';
import type { FlConnectView, FlGopherStatus, FlLinkStatus, FlLogEntry, Snapshot } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { DesktopIcon } from './apps.tsx';
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

const MIDI_LABEL: Record<FlLinkStatus, { text: string; tone: 'ok' | 'bad' | 'warn' | 'muted' }> = {
  off: { text: '止まっている', tone: 'muted' },
  'no-port': { text: 'ポートが無い', tone: 'bad' },
  silent: { text: 'FL が応答しない', tone: 'warn' },
  connected: { text: 'つながっている', tone: 'ok' },
};

const GOPHER_LABEL: Record<FlGopherStatus, { text: string; tone: 'ok' | 'bad' | 'warn' | 'muted' }> = {
  off: { text: '使わない', tone: 'muted' },
  'no-port': { text: 'ポートが無い', tone: 'warn' },
  'no-page': { text: 'Gopher が開いていない', tone: 'warn' },
  'no-handler': { text: '口が無い', tone: 'bad' },
  connected: { text: 'つながっている', tone: 'ok' },
  busy: { text: 'Gopher が使っている', tone: 'warn' },
};

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

const Status = ({ label, status, message }: { label: string; status: { text: string; tone: 'ok' | 'bad' | 'warn' | 'muted' }; message?: string }) => (
  <div class="fl-status">
    <span class="fl-status-label">{label}</span>
    <Chip tone={status.tone}>{status.text}</Chip>
    {message && <span class="fl-status-message">{message}</span>}
  </div>
);

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

const LogItem = ({ e }: { e: FlLogEntry }) => (
  <li class={`fl-log-item${e.ok ? '' : ' is-failed'}`}>
    <details>
      <summary>
        <time>{clock(e.at)}</time> <code>{e.tool}</code> {e.title}
        {e.changes?.length ? <span class="fl-log-count">{e.changes.length} 件の変更</span> : null}
      </summary>
      {e.detail && <pre class="fl-pre">{e.detail}</pre>}
      {e.output && <pre class="fl-pre">{e.output}</pre>}
      {e.changes?.length ? (
        <ul class="fl-changes">
          {e.changes.map((c, k) => <li key={k}>{c}</li>)}
        </ul>
      ) : null}
    </details>
  </li>
);

/** 導入の手順ごとに、済んだかを並べる。 */
const Setup = ({ v }: { v: FlConnectView }) => {
  const [message, setMessage] = useState<string>();
  const run = (label: string, fn: () => Promise<{ message: string }>) => void attempt(label, fn).then((r) => r && setMessage(r.message));
  const scriptOk = !!v.midi.hello && v.midi.hello.script === v.setup.bundled && !v.midi.hello.coreError;
  const steps: [boolean, string][] = [
    [v.midi.status !== 'no-port' && v.midi.status !== 'off', `MIDI のループバック（${v.midi.ports.output}）がある`],
    [v.setup.installed === v.setup.bundled, v.setup.userData ? `スクリプト ${v.setup.bundled} 版を FL のユーザーデータに置いた（今は ${v.setup.installed ?? '置いていない'}）` : 'FL のユーザーデータのフォルダーが見つかった'],
    [!!v.midi.hello, 'FL が FL Connect のスクリプトを動かして名乗った'],
    [scriptOk, 'FL が動かしているスクリプトの版が合う'],
    [v.gopher.status === 'connected', '② の道（Gopher）につながった（② を使うときだけ）'],
  ];
  return (
    <details class="fl-setup" open={!scriptOk}>
      <summary>導入</summary>
      <ul class="fl-steps">
        {steps.map(([ok, text]) => (
          <li key={text} class={ok ? 'is-done' : ''}>
            <span aria-hidden="true">{ok ? '✔' : '・'}</span> {text}
          </li>
        ))}
      </ul>
      {v.setup.userData && <p class="fl-note">FL のユーザーデータ: {v.setup.userData}</p>}
      <div class="fl-actions">
        <button class="btn btn-sm" onClick={() => run('スクリプトを置く', api.flInstall)}>スクリプトを置く</button>
        <button class="btn btn-sm" onClick={() => run('ループバックを作る', api.flLoopback)}>ループバックを作る</button>
        <button class="btn btn-sm" title="② の道のため、デバッグのポートを開く設定で FL を起動します" onClick={() => run('FL を起動する', api.flLaunch)}>
          ② のために FL を起動
        </button>
      </div>
      <p class="fl-note">
        FL の MIDI の設定で、入力の FL Connect (A) を有効にして Controller type に「FL Connect」を選び、出力の FL Connect (A) に同じポートの番号を付けます。番号はほかの機器と重ならないものにします。
      </p>
      {message && <pre class="fl-pre">{message}</pre>}
    </details>
  );
};

export const FlConnectFloat = ({ f, snapshot }: { f: Float; snapshot: Snapshot }) => {
  // つなぎ直すときに、そのときのセッションを読めるよう、ref に持つ。
  const sessionRef = useRef(f.tab.settings?.session);
  const v = useFlView(() => sessionRef.current);
  const [tab, setTab] = useState<'log' | 'records'>('records');
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
            <Status label="① MIDI スクリプト" status={MIDI_LABEL[v.midi.status]} message={v.midi.status === 'connected' ? (v.midi.message ?? `FL ${v.midi.hello?.fl ?? ''}、スクリプト ${v.midi.hello?.script ?? ''}`) : v.midi.message} />
            <Status label="② Gopher の口" status={GOPHER_LABEL[v.gopher.status]} message={v.gopher.enabled ? (v.gopher.status === 'connected' ? `ツール ${v.gopher.tools ?? '?'} 個` : v.gopher.message) : undefined} />
            <div class="fl-toggles">
              <label title="FL 2026 の内蔵の AI の Gopher が使う口を、エージェントにも使わせます。デバッグのポートを開いて起動した FL で、Gopher を開くとつながります">
                <input type="checkbox" checked={v.gopher.enabled} onChange={(e) => void attempt('② の道の切り替え', () => api.flSettings({ gopher: e.currentTarget.checked }))} /> ② の道を使う（FL 2026 の Gopher の口）
              </label>
              <label title="入れている間、エージェントは FL を読むだけで、値を変えられません">
                <input type="checkbox" checked={v.readOnly} onChange={(e) => void attempt('書き込みの切り替え', () => api.flSettings({ readOnly: e.currentTarget.checked }))} /> 書き込みを止める
              </label>
            </div>
            {v.gopher.enabled && v.gopher.status === 'connected' && <p class="fl-note fl-warn">デバッグのポートが開いている間は、この PC のどのプロセスからも FL を操作できます。使い終えたら、FL をふつうに起動し直してください。</p>}
            {v.focus && (
              <p class="fl-focus">
                {v.focus.playing ? '▶ 再生中' : '■ 停止'}　{v.focus.tempo ?? '?'} BPM　前面: {v.focus.window ?? v.focus.caption ?? '?'}　channel {v.focus.channel}、mixer {v.focus.mixer}、pattern {v.focus.pattern}　元に戻す履歴 {v.focus.undo}
              </p>
            )}
            <Setup v={v} />
            <div class="fl-tabs" role="tablist">
              <button role="tab" aria-selected={tab === 'records'} class={`btn btn-sm${tab === 'records' ? ' is-active' : ''}`} onClick={() => setTab('records')}>
                操作の記録
              </button>
              <button role="tab" aria-selected={tab === 'log'} class={`btn btn-sm${tab === 'log' ? ' is-active' : ''}`} onClick={() => setTab('log')}>
                エージェントがしたこと
              </button>
            </div>
            {tab === 'records' ? (
              v.records.length ? (
                <ul class="fl-records">
                  {[...v.records].reverse().map((r) => (
                    <li key={r.seq} class={`fl-record is-${r.who}`}>
                      <time>{clock(r.at)}</time> <span class="fl-who">{r.who === 'human' ? '人間' : (r.via ?? 'エージェント')}</span> {r.text}
                    </li>
                  ))}
                </ul>
              ) : (
                <Empty>FL で名前、色、音量などを変えると、ここに並びます。</Empty>
              )
            ) : v.log.length ? (
              <ul class="fl-log">
                {[...v.log].reverse().map((e) => <LogItem key={e.id} e={e} />)}
              </ul>
            ) : (
              <Empty>エージェントが FL を操作すると、動かしたコードと出力と、変わった値がここに並びます。</Empty>
            )}
          </>
        )}
      </div>
    </FloatFrame>
  );
};
