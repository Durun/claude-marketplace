// アプリ「ムービーメーカー」のウィンドウ。movie-maker を入れて起動し、開いている間だけ、そのプロジェクトをつないだセッションのエージェントが操作できる。
// サーバーとは SSE でつなぎ、入れ方、つながり、ログを受け取る。ダッシュボードが起動し直して切れたら、つなぎ直す。
import { useEffect, useRef, useState } from 'preact/hooks';
import type { MmLogEntry, MovieMakerView, Snapshot } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { DesktopIcon } from './apps.tsx';
import { openFileFloat } from './Docs.tsx';
import { type Float, FloatFrame, dock, floatUp, setFloatSettings } from './Float.tsx';
import { describeMmEntry, type MmAction, type MmStep, mmSteps } from './mmSteps.ts';
import { Chip, Empty } from './ui.tsx';
import { XpConfirm } from './XpConfirm.tsx';

/** ウィンドウの保存キーで、ダッシュボードに置くアプリの種類の名前でもある。movie-maker とのつながりは 1 組なので、ウィンドウは 1 つだけ開く。 */
export const MM_KEY = 'moviemaker';
export const MM_TITLE = 'ムービーメーカー';
export const MM_ICON = '/moviemaker.svg';
const SIZE = { w: 640, h: 600 };
const RECONNECT_MS = 2000;

/** セッションの画面で開いたら、そのセッションにつなぐ。 */
export const openMovieMaker = (sessionId?: string): void => {
  floatUp(MM_KEY, '', { id: MM_KEY, settings: sessionId ? { session: sessionId } : undefined }, SIZE);
  if (sessionId) void attempt('ムービーメーカーのセッションの切り替え', () => api.mmSession(sessionId));
};

export const MovieMakerIcon = ({ sessionId }: { sessionId?: string }) => (
  <DesktopIcon icon={MM_ICON} title={MM_TITLE} hint="movie-maker を起動し、エージェントが movie-maker の操作を知り、プロジェクトを編集できるようにします。開いている間だけつながります。ダブルクリックで開きます" onLaunch={() => openMovieMaker(sessionId)} />
);

const clock = (at: number): string => new Date(at).toTimeString().slice(0, 8);

/** サーバーに SSE でつなぎ、届いた状態を返す。つなぎ直すときは、そのときのセッションとプロジェクトを渡す。 */
const useMmView = (params: () => { session?: string; project?: string }): MovieMakerView | undefined => {
  const [view, setView] = useState<MovieMakerView>();
  useEffect(() => {
    let source: EventSource | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let live = true;
    const connect = (): void => {
      const { session, project } = params();
      source = new EventSource(api.mmEventsUrl(session, project));
      source.addEventListener('view', (e) => setView(JSON.parse((e as MessageEvent<string>).data) as MovieMakerView));
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

const ACTIONS: Record<MmAction, () => Promise<{ message: string }>> = { install: api.mmInstall, launch: api.mmLaunch };

const Steps = ({ steps }: { steps: MmStep[] }) => {
  const [message, setMessage] = useState<string>();
  const run = (step: MmStep) => {
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

/** プロジェクトのフォルダー。入力して「開く」で、サーバーのつなぐ先を切り替える。 */
const ProjectField = ({ project }: { project?: string }) => {
  const [draft, setDraft] = useState(project ?? '');
  useEffect(() => setDraft(project ?? ''), [project]);
  const open = () => void attempt('プロジェクトの切り替え', () => api.mmProject(draft.trim() || undefined));
  return (
    <form class="inline-field mm-project" onSubmit={(e) => (e.preventDefault(), open())}>
      プロジェクト
      <input class="input input-sm" value={draft} placeholder="D:\videos\ep04（project.mmdb のあるフォルダー）" onInput={(e) => setDraft(e.currentTarget.value)} />
      <button class="btn btn-sm" type="submit" disabled={draft.trim() === (project ?? '')}>
        開く
      </button>
    </form>
  );
};

const WHO = { you: 'You', agent: 'Agent' } as const;

const LogItem = ({ e }: { e: MmLogEntry }) => (
  <li class={`fl-log-item is-${e.who}${e.who === 'agent' && !e.ok ? ' is-failed' : ''}`}>
    <details>
      <summary>
        <time>{clock(e.at)}</time> <span class="fl-who">{WHO[e.who]}</span> {describeMmEntry(e)}
      </summary>
      {e.who === 'you' ? (
        <pre class="fl-pre">{JSON.stringify(e.change)}</pre>
      ) : (
        <>
          <pre class="fl-pre">{JSON.stringify(e.args, null, 2)}</pre>
          {e.output && <pre class="fl-pre">{e.output}</pre>}
        </>
      )}
    </details>
  </li>
);

const Log = ({ v }: { v: MovieMakerView }) => {
  const entries = [...v.log].reverse();
  return (
    <>
      <div class="fl-log-bar">
        <span class="fl-log-title">ログ</span>
        {v.logFile && v.sessionId && (
          <a class="fl-log-file" href="#" title={`${v.logFile}。セッションの資料にも足してあります`} onClick={(e) => {
            e.preventDefault();
            openFileFloat(v.sessionId ?? '', v.logFile ?? '');
          }}>
            ファイルで開く
          </a>
        )}
      </div>
      {entries.length ? <ul class="fl-log">{entries.map((e) => <LogItem key={e.id} e={e} />)}</ul> : <Empty>movie-maker で操作するか、エージェントが movie-maker を操作すると、ここに並びます。</Empty>}
    </>
  );
};

const LinkChips = ({ v }: { v: MovieMakerView }) => (
  <div class="fl-links">
    {v.install.path ? <Chip tone="ok" title={v.install.path}>入っている{v.install.commit ? `（${v.install.commit}${v.install.features?.length ? `、${v.install.features.join('、')}` : ''}）` : ''}</Chip> : <Chip tone="muted">入っていない</Chip>}
    {v.link.status === 'connected' ? <Chip tone="ok" title={`127.0.0.1:${v.link.port}`}>movie-maker とつながっている（version {v.link.version ?? '?'}）</Chip> : <Chip tone="muted">movie-maker とつながっていない</Chip>}
  </div>
);

export const MovieMakerFloat = ({ f, snapshot }: { f: Float; snapshot: Snapshot }) => {
  const settingsRef = useRef({ session: f.tab.settings?.session, project: f.tab.settings?.project });
  const v = useMmView(() => settingsRef.current);
  const bound = v?.sessionId;
  const project = v?.project;
  // サーバーで切り替えたセッションとプロジェクトをウィンドウの設定に残し、ダッシュボードが起動し直したときも同じものにつなぐ。
  const received = !!v;
  useEffect(() => {
    if (!received) return;
    settingsRef.current = { session: bound, project };
    const { session: _s, project: _p, ...rest } = f.tab.settings ?? {};
    const next = { ...rest, ...(bound ? { session: bound } : {}), ...(project ? { project } : {}) };
    if (next.session !== f.tab.settings?.session || next.project !== f.tab.settings?.project) setFloatSettings(f, next);
  }, [received, bound, project]);
  const sessions = snapshot.sessions.filter((s) => !s.archived || s.id === bound);
  const nameOf = (id: string): string => snapshot.sessions.find((s) => s.id === id)?.name ?? id.slice(0, 8);
  const [closing, setClosing] = useState<string>();
  const closeWindow = () => dock(f.key, f.scope, f.tab.id);
  const close = { kind: 'close' as const, label: '閉じる', onClick: () => (bound ? setClosing(nameOf(bound)) : closeWindow()) };
  const answerClose = (ok: boolean) => {
    setClosing(undefined);
    if (ok) closeWindow();
  };
  return (
    <FloatFrame f={f} title={bound ? `${nameOf(bound)} - ${MM_TITLE}` : MM_TITLE} icon={MM_ICON} controls={[close]}>
      <div class="widget-body fl-connect">
        {!v ? (
          <Empty>ダッシュボードにつないでいます…</Empty>
        ) : (
          <>
            <label class="inline-field fl-session">
              つなぐセッション
              <select class="input input-sm input-auto" value={bound ?? ''} onChange={(e) => void attempt('ムービーメーカーのセッションの切り替え', () => api.mmSession(e.currentTarget.value || undefined))}>
                <option value="">（選んでいない。どのセッションにも届けない）</option>
                {sessions.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}
                  </option>
                ))}
              </select>
            </label>
            <ProjectField project={project} />
            <LinkChips v={v} />
            <Steps steps={mmSteps(v)} />
            {(v.install.busy || v.install.output.length > 0) && <pre class="fl-pre mm-output">{v.install.output.join('\n')}</pre>}
            <Log v={v} />
          </>
        )}
      </div>
      {closing !== undefined && <XpConfirm title="終了の確認" text={`ムービーメーカーを閉じると、セッション「${closing}」とのつながりが切れ、エージェントは movie-maker を操作できなくなります。閉じますか?`} onClose={answerClose} />}
    </FloatFrame>
  );
};
