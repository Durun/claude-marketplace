import { useEffect, useRef, useState } from 'preact/hooks';
import { EFFORT_LEVELS, MODELS, type Session, type Snapshot } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { HOME_KEY, layoutKeyOf, useDashboards } from './dashboards.ts';
import { Dashboard, type EditProps, type Widget, type WidgetProps, WidgetPanel } from './Dashboard.tsx';
import { DesktopIcon } from './apps.tsx';
import { EXPLORER_ICON, EXPLORER_KEY, EXPLORER_TITLE, ExplorerIcon, openExplorer } from './Explorer.tsx';
import { NOTEPAD_ICON, NOTEPAD_KEY, NOTEPAD_TITLE, NotepadIcon, openNotepad } from './Notepad.tsx';
import { type Float, FloatFrame, GITHUB_ICON, JIRA_ICON, FloatWindow, dock, floatUp, isFloating, useFloats } from './Float.tsx';
import { GITHUB_INBOX_TITLE, GithubInbox, githubInboxControls } from './GithubInbox.tsx';
import { Inbox } from './Inbox.tsx';
import { JIRA_TITLE, JiraIssues, jiraControls } from './JiraIssues.tsx';
import { ROWS, type Layout } from './layout.ts';
import { MessageInput } from './Messaging.tsx';
import { ThemeGroups, type Selection } from './SessionCards.tsx';
import { useWaitCursor } from './hooks.ts';
import { toast } from './toast.tsx';
import { Choice } from './ui.tsx';

const SelectionBar = ({ ids, onClear }: { ids: string[]; onClear: () => void }) => {
  const [busy, setBusy] = useState(false);
  const archive = async () => {
    setBusy(true);
    const done = await attempt('アーカイブ', () => Promise.all(ids.map((id) => api.archive(id, true))));
    setBusy(false);
    if (!done) return;
    toast(`${ids.length} 件をアーカイブしました`);
    onClear();
  };
  return (
    <div class="selection-bar" role="toolbar" aria-label="選んだセッションへの操作">
      <span><b>{ids.length}</b> 件を選択中</span>
      <button class="btn btn-sm btn-primary" disabled={busy} onClick={archive}>アーカイブする</button>
      <button class="btn btn-sm" onClick={onClear}>選択を解除</button>
    </div>
  );
};

/** ディレクトリの候補は、これまでのセッションの作業ディレクトリを最近使った順に並べる。 */
const recentCwds = (sessions: Session[]): string[] =>
  [...new Set(sessions.toSorted((a, b) => b.updatedAt - a.updatedAt).map((s) => s.cwd))];

/**
 * 新規セッションの入力欄の値を、開き直しても前回の選択から始められるようブラウザに保存する。
 * options を渡すと、選択肢から消えた値は既定に戻す。
 */
const useSaved = (key: string, options?: readonly string[]) => {
  const [value, setValue] = useState(() => {
    const v = localStorage.getItem(key) ?? '';
    return !options || options.includes(v) ? v : '';
  });
  const save = (next: string) => {
    setValue(next);
    localStorage.setItem(key, next);
  };
  return [value, save] as const;
};

// 新規セッションのウィンドウを開くときに、ディレクトリの欄へ入れる値。ウィンドウが開いていれば、入力欄へ直に渡す。
let presetCwd: string | undefined;
const presetListeners = new Set<(cwd: string) => void>();

const NewSession = ({ cwds, onClose }: { cwds: string[]; onClose: () => void }) => {
  const dir = useRef<HTMLInputElement>(null);
  const ask = useRef<HTMLTextAreaElement>(null);
  const [cwd, setCwd] = useSaved('dashboard-new-session-cwd');
  const [prompt, setPrompt] = useState('');
  const [model, setModel] = useSaved('dashboard-new-session-model', MODELS);
  const [effort, setEffort] = useSaved('dashboard-new-session-effort', EFFORT_LEVELS);
  const [busy, setBusy] = useState(false);
  useWaitCursor(busy);
  // セッションの画面から開いたときは、そのセッションの作業ディレクトリを入れる。開いている間にもう一度開かれたら、入れ直す。
  // ディレクトリが埋まっていれば、次に書く最初の依頼から入力できるようにする。
  useEffect(() => {
    const preset = presetCwd;
    presetCwd = undefined;
    if (preset) setCwd(preset);
    (preset || cwd ? ask : dir).current?.focus();
    presetListeners.add(setCwd);
    return () => {
      presetListeners.delete(setCwd);
    };
  }, []);
  const ready = !!cwd.trim() && !!prompt.trim();
  const start = async () => {
    if (busy || !ready) return;
    setBusy(true);
    const done = await attempt('セッションの起動', () => api.startSession(cwd.trim(), prompt, model, effort));
    setBusy(false);
    if (!done) return;
    toast('セッションを起動しました');
    onClose();
  };
  return (
    <div class="stack new-session">
      <label class="inline-field">
        <span class="nowrap">ディレクトリ</span>
        <input ref={dir} class="input mono" list="cwd-options" placeholder="/path/to/repo" value={cwd} onInput={(e) => setCwd(e.currentTarget.value)} />
        <datalist id="cwd-options">
          {cwds.map((c) => <option key={c} value={c} />)}
        </datalist>
      </label>
      <MessageInput value={prompt} setValue={setPrompt} placeholder="最初の依頼（Cmd/Ctrl + Enter で起動）" onSubmit={start} inputRef={ask} />
      <div class="actions">
        <Choice label="モデル" value={model} options={MODELS} onChange={setModel} />
        <Choice label="effort" value={effort} options={EFFORT_LEVELS} onChange={setEffort} />
      </div>
      <div class="actions">
        <span class="help">バックグラウンドで Claude Code を起動し、最初の依頼を渡します。</span>
        <span class="spacer" />
        <button class="btn" onClick={onClose}>キャンセル</button>
        <button class="btn btn-primary" disabled={busy || !ready} onClick={start}>起動</button>
      </div>
    </div>
  );
};

/** 新規セッションのウィンドウの保存キー。ホームのウィジェットではないので、scope は空にし、タブの id にもこのキーを入れる。 */
export const NEW_SESSION_KEY = 'new-session';
const NEW_SESSION_TAB = { id: NEW_SESSION_KEY };
const closeNewSession = () => dock(NEW_SESSION_KEY, '', NEW_SESSION_KEY);
// 入力欄がちょうど収まる大きさ（px）で、画面の上寄りの中ほどに出す。
const NEW_SESSION_SIZE = { w: 640, h: 290 };
export const NEW_SESSION_TITLE = '新規セッション';
export const NEW_SESSION_ICON = '/claude-new.svg';

/** 新規セッションのウィンドウを開く。開いていれば手前に出す。cwd を渡すと、ディレクトリの欄にそれを入れる。 */
export const openNewSession = (cwd?: string): void => {
  if (cwd && presetListeners.size > 0) presetListeners.forEach((fn) => fn(cwd));
  else presetCwd = cwd;
  floatUp(NEW_SESSION_KEY, '', NEW_SESSION_TAB, NEW_SESSION_SIZE);
};

/** アプリ「新規セッション」のアイコン。cwd は、セッションの画面に置いたときの、そのセッションの作業ディレクトリ。 */
export const NewSessionIcon = ({ cwd }: { cwd?: string }) => (
  <DesktopIcon
    icon={NEW_SESSION_ICON}
    title={NEW_SESSION_TITLE}
    hint={cwd ? `ダブルクリックで、${cwd} で始めるセッションの入力欄を開きます` : 'ダブルクリックで、新しいセッションの入力欄を開きます'}
    onLaunch={() => openNewSession(cwd)}
  />
);

export const NewSessionFloat = ({ f, snapshot }: HomeData & { f: Float }) => (
  <FloatFrame f={f} title={NEW_SESSION_TITLE} icon={NEW_SESSION_ICON} controls={[{ kind: 'close', label: '閉じる', onClick: closeNewSession }]}>
    <NewSession cwds={recentCwds(snapshot.sessions)} onClose={closeNewSession} />
  </FloatFrame>
);

/** ホームのウィジェットに渡すデータ。 */
type HomeData = { snapshot: Snapshot };
export type HomeWidgetProps = WidgetProps<HomeData>;

const InboxWidget = ({ snapshot }: HomeWidgetProps) => (
  <WidgetPanel title="要対応" count={snapshot.attention.length}>
    <Inbox snapshot={snapshot} />
  </WidgetPanel>
);

const SessionsWidget = ({ snapshot }: HomeWidgetProps) => {
  const creating = isFloating(useFloats(), NEW_SESSION_KEY, '', NEW_SESSION_KEY);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const sel: Selection = {
    selected,
    toggle: (ids, on) =>
      setSelected((prev) => {
        const next = new Set(prev);
        for (const id of ids) on ? next.add(id) : next.delete(id);
        return next;
      }),
  };
  // 一覧から消えたセッションは、選んだ数にも操作の対象にも入れない。
  const ids = snapshot.sessions.filter((s) => selected.has(s.id)).map((s) => s.id);
  return (
    <>
      <div class="section-head">
        <h2 class="section-title">セッション</h2>
        <div class="actions">
          <button class="btn btn-sm btn-primary" data-icon="add" aria-expanded={creating} onClick={() => openNewSession()}>新規セッション</button>
        </div>
      </div>
      <div class={`widget-body${ids.length ? ' is-selecting' : ''}`}>
        <ThemeGroups snapshot={snapshot} sel={sel} />
        {ids.length > 0 && <SelectionBar ids={ids} onClear={() => setSelected(new Set())} />}
      </div>
    </>
  );
};

const WIDGETS: Record<string, Widget<HomeData>> = {
  inbox: { title: '要対応', icon: '📥', Body: InboxWidget, size: { w: 10, h: ROWS }, sketch: 'list' },
  sessions: { title: 'セッション', icon: '🗂️', Body: SessionsWidget, size: { w: 14, h: ROWS }, sketch: 'cards' },
  jira: { title: JIRA_TITLE, icon: JIRA_ICON, Body: JiraIssues, size: { w: 8, h: ROWS / 2 }, sketch: 'list', controls: jiraControls },
  github: { title: GITHUB_INBOX_TITLE, icon: GITHUB_ICON, Body: GithubInbox, size: { w: 8, h: ROWS / 2 }, sketch: 'list', controls: githubInboxControls },
  [EXPLORER_KEY]: { title: EXPLORER_TITLE, icon: EXPLORER_ICON, Body: () => <ExplorerIcon />, size: { w: 2, h: 3 }, sketch: 'cards', launch: () => openExplorer() },
  [NOTEPAD_KEY]: { title: NOTEPAD_TITLE, icon: NOTEPAD_ICON, Body: () => <NotepadIcon />, size: { w: 2, h: 3 }, sketch: 'cards', launch: () => openNotepad() },
  newSession: { title: NEW_SESSION_TITLE, icon: NEW_SESSION_ICON, Body: () => <NewSessionIcon />, size: { w: 2, h: 3 }, sketch: 'cards', launch: () => openNewSession() },
};

// 初期配置には、JIRA の設定が無くても使える種類を 1 つずつ入れる。
const DEFAULT_LAYOUT: Layout = [
  { id: 'inbox', x: 0, y: 0, w: 10, h: ROWS, tabs: [{ id: 'inbox' }] },
  { id: 'sessions', x: 10, y: 0, w: 14, h: ROWS, tabs: [{ id: 'sessions' }] },
];

export const Home = ({ snapshot, editing, onEditDone }: HomeData & EditProps) => {
  const layoutKey = layoutKeyOf(useDashboards().current);
  // 切り替えたダッシュボードの配置を読み直すよう、key で作り直す。
  return <Dashboard key={layoutKey} widgets={WIDGETS} defaults={DEFAULT_LAYOUT} storageKey={HOME_KEY} layoutKey={layoutKey} data={{ snapshot }} editing={editing} onEditDone={onEditDone} />;
};

export const HomeFloat = ({ f, snapshot }: HomeData & { f: Float }) => <FloatWindow f={f} widgets={WIDGETS} data={{ snapshot }} />;
