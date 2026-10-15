import type { ComponentChildren } from 'preact';
import { useEffect, useMemo, useState } from 'preact/hooks';
import type { Attention, ChatEntry, DocMeta, OutboxMessage, Session, Snapshot } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { CodeView } from './CodeView.tsx';
import { Dashboard, type EditProps, type Widget, type WidgetFocus, WidgetPanel } from './Dashboard.tsx';
import { SessionDocs } from './Docs.tsx';
import { EXPLORER_ICON, EXPLORER_KEY, EXPLORER_TITLE, ExplorerIcon, openExplorer } from './Explorer.tsx';
import { IE_ICON, IE_KEY, IE_TITLE, InternetExplorerIcon, openInternetExplorer } from './InternetExplorer.tsx';
import { NOTEPAD_ICON, NOTEPAD_KEY, NOTEPAD_TITLE, NotepadIcon, openNotepad } from './Notepad.tsx';
import { PROOFREAD_ICON, PROOFREAD_KEY, PROOFREAD_TITLE, ProofreadIcon, openProofread } from './Proofread.tsx';
import { TERMINAL_ICON, TERMINAL_KEY, TERMINAL_TITLE, TerminalIcon, openTerminal } from './Terminal.tsx';
import { type Control, type Float, FloatWindow, GITHUB_ICON, JIRA_ICON } from './Float.tsx';
import { NEW_SESSION_ICON, NEW_SESSION_TITLE, NewSessionIcon, openNewSession } from './Home.tsx';
import { useFetch, useWaitCursor } from './hooks.ts';
import { COLS, ROWS, type Layout } from './layout.ts';
import { OutboxList, asksOf } from './Messaging.tsx';
import { AgentList, Chat, DIAGRAM_ICON, DiagramIcon, type Jump, REMOTE_ICON, REMOTE_TITLE, RemoteIcon, Summary, TaskList, launchDiagram, openRemote } from './Overview.tsx';
import { PR_REVIEW_TITLE, PrReview } from './PrReview.tsx';
import { JIRA_VIEW_TITLE, JIRA_WIDGET, JiraWidget } from './JiraView.tsx';
import { PR_TITLE, PR_WIDGET, PrWidget } from './PrView.tsx';
import { type SessionTab, href } from './router.ts';
import { CopyButton, CopyText, Empty, IssueChip, JiraChips, PrChips, StatusBadge } from './ui.tsx';
import { UsageView } from './Usage.tsx';

const ThemeEditor = ({ s, themes }: { s: Session; themes: string[] }) => {
  const [value, setValue] = useState(s.theme);
  useEffect(() => setValue(s.theme), [s.theme]);
  // change は入力を確定したとき（フォーカス移動か Enter）にだけ発火する。
  // 候補の選択や自動入力では input と同時に来るため、state ではなく入力欄の値を読む。
  const commit = (input: HTMLInputElement) => {
    const theme = input.value.trim();
    if (theme !== s.theme) attempt('テーマの変更', () => api.theme(s.id, theme));
  };
  return (
    <label class="inline-field">
      テーマ
      <input
        class="input input-sm"
        list="theme-options"
        value={value}
        placeholder="未分類"
        onInput={(e) => setValue(e.currentTarget.value)}
        onChange={(e) => commit(e.currentTarget)}
      />
      <datalist id="theme-options">
        {themes.map((t) => <option key={t} value={t} />)}
      </datalist>
    </label>
  );
};

/** 名前は Enter かフォーカスを外したときに確定する。空のまま Enter を押すと、提案された名前を入力欄に入れる。 */
const NameEditor = ({ s }: { s: Session }) => {
  const [value, setValue] = useState(s.name);
  const [thinking, setThinking] = useState(false);
  useEffect(() => setValue(s.name), [s.name]);
  const commit = (input: HTMLInputElement) => {
    const title = input.value.trim();
    if (!title) setValue(s.name);
    else if (title !== s.name) attempt('名前の変更', () => api.title(s.id, title));
  };
  const propose = async (input: HTMLInputElement) => {
    setThinking(true);
    const res = await attempt('名前の提案', () => api.suggestTitle(s.id));
    setThinking(false);
    // 待つ間に入力欄を離れていたら、確定されない名前が表示に残るので入れない。
    if (res && document.activeElement === input) setValue(res.title);
  };
  const onKeyDown = (e: KeyboardEvent) => {
    const input = e.currentTarget as HTMLInputElement;
    if (e.isComposing) return;
    if (e.key === 'Escape') {
      // フォーカスを外すと確定するので、先に入力欄の値を戻しておく。
      input.value = s.name;
      setValue(s.name);
      input.blur();
    } else if (e.key === 'Enter' && input.value.trim()) {
      input.blur();
    } else if (e.key === 'Enter' && !thinking) {
      void propose(input);
    }
  };
  return (
    <h1 class="session-name">
      <input
        class="name-input"
        value={value}
        readOnly={thinking}
        aria-label="セッション名"
        title="名前を変えられます。空にして Enter を押すと、名前を提案します。"
        placeholder={thinking ? '名前を考えています…' : '空のまま Enter で名前を提案します'}
        onInput={(e) => setValue(e.currentTarget.value)}
        onKeyDown={onKeyDown}
        onBlur={(e) => commit(e.currentTarget)}
      />
    </h1>
  );
};

/** アーカイブの切り替えは、タイトルバー右端のボタンの枠に並べる。 */
const archiveControl = ({ s }: { s: Session }): Control[] => [
  {
    kind: 'archive',
    label: s.archived ? 'アーカイブを解除' : 'アーカイブする',
    pressed: s.archived,
    onClick: () => void attempt('アーカイブの切り替え', () => api.archive(s.id, !s.archived)),
  },
];

/** バッジの右の × で、バッジを外す。外したバッジは、ブランチや set_task から見つけ直しても出さない。 */
const Removable = ({ s, url, children }: { s: Session; url: string; children: ComponentChildren }) => (
  <span class="badge-item">
    {children}
    <button class="badge-remove" title="バッジを外す" aria-label="バッジを外す" onClick={() => void attempt('バッジの取り外し', () => api.removeBadge(s.id, url))}>×</button>
  </span>
);

/** PR、Issue、JIRA の課題の URL を貼って Enter を押すと、バッジに登録する。 */
const BadgeInput = ({ s }: { s: Session }) => {
  const [value, setValue] = useState('');
  const [busy, setBusy] = useState(false);
  const add = async (e: Event) => {
    e.preventDefault();
    const url = value.trim();
    if (!url || busy) return;
    setBusy(true);
    // PR と Issue は、サーバーが GitHub から読めると確かめてから登録するので、数秒待つことがある。
    const ok = await attempt('バッジの登録', async () => (await api.addBadge(s.id, url), true));
    setBusy(false);
    if (ok) setValue('');
  };
  return (
    <form class="badge-add" onSubmit={add}>
      <input
        class="input input-sm"
        value={value}
        readOnly={busy}
        aria-label="バッジに登録する URL"
        title="PR、Issue、JIRA の課題の URL を貼って Enter を押すと、バッジに登録します。"
        placeholder={busy ? '登録しています…' : 'PR、Issue、JIRA の URL'}
        onInput={(e) => setValue(e.currentTarget.value)}
      />
    </form>
  );
};

/** 名前の行をウィジェットの見出しにする。 */
const SessionHead = ({ s, themes }: { s: Session; themes: string[] }) => (
  <>
    <div class="head-row session-title">
      <NameEditor key={s.id} s={s} />
      <StatusBadge status={s.status} />
    </div>
    <div class="widget-body session-head">
      <div class="head-row">
        <ThemeEditor s={s} themes={themes} />
        <span class="mono muted">
          <CopyText text={s.cwd} title={`${s.cwd}（押すと絶対パスをコピー）`}>{s.repo}</CopyText>
          {s.branch && <> · <CopyText text={s.branch} title="押すとブランチ名をコピー">{s.branch}</CopyText></>}
        </span>
      </div>
      <div class="head-row">
        {/* 前に開いていたときのデータには、PR の一覧が無いことがある。 */}
        {s.prs?.map((pr) => <Removable key={pr.url} s={s} url={pr.url}><PrChips pr={pr} session={s.id} /></Removable>)}
        {s.jira.map((j) => <Removable key={j.key} s={s} url={j.url}><JiraChips issues={[j]} session={s.id} /></Removable>)}
        {s.issues?.map((i) => <Removable key={i.url} s={s} url={i.url}><IssueChip issue={i} /></Removable>)}
        <BadgeInput s={s} />
        {s.attachCommand && (
          <span class="attach">
            <code title="ターミナルで開くコマンド">{s.attachCommand}</code>
            <CopyButton text={s.attachCommand} label="コピー" title="ターミナルで開くコマンドをコピー" />
          </span>
        )}
      </div>
    </div>
  </>
);

/** file は、コードの画面へのリンクで開いたファイル。 */
const CodeWidget = ({ s, file }: { s: Session; file?: string }) => (
  <WidgetPanel title="コード">
    <CodeView s={s} focus={file} />
  </WidgetPanel>
);

const UsageWidget = ({ s }: { s: Session }) => (
  <WidgetPanel title="使用量">
    <UsageView s={s} />
  </WidgetPanel>
);

/** ウィジェットに渡す、このセッションのデータ。docs は present で出した資料、entries はチャットの発言で、jump はそこへ移る先。 */
type SessionData = {
  s: Session;
  themes: string[];
  file?: string;
  outbox: OutboxMessage[];
  docs: DocMeta[];
  entries: ChatEntry[];
  asks: Attention[];
  jump?: Jump;
  onJump: (index: number) => void;
  /** Notion のトークンを設定してあるか。 */
  notion: boolean;
};

const WIDGETS: Record<string, Widget<SessionData>> = {
  head: { title: 'セッション', icon: '🖥️', Body: SessionHead, size: { w: 10, h: 6 }, sketch: 'text', controls: archiveControl },
  summary: { title: '概要', icon: '📝', Body: Summary, size: { w: 8, h: 6 }, sketch: 'text' },
  diagram: { title: '図解', icon: DIAGRAM_ICON, Body: DiagramIcon, size: { w: 2, h: 3 }, sketch: 'cards', launch: ({ s }) => launchDiagram(s) },
  // セッションの画面から起動した新規セッションは、そのセッションの作業ディレクトリとテーマで始める。
  newSession: { title: NEW_SESSION_TITLE, icon: NEW_SESSION_ICON, Body: ({ s }) => <NewSessionIcon cwd={s.cwd} theme={s.theme} />, size: { w: 2, h: 3 }, sketch: 'cards', launch: ({ s }) => openNewSession(s.cwd, s.theme) },
  [EXPLORER_KEY]: { title: EXPLORER_TITLE, icon: EXPLORER_ICON, Body: ({ s }) => <ExplorerIcon cwd={s.cwd} />, size: { w: 2, h: 3 }, sketch: 'cards', launch: ({ s }) => openExplorer(s.cwd) },
  [IE_KEY]: { title: IE_TITLE, icon: IE_ICON, Body: InternetExplorerIcon, size: { w: 2, h: 3 }, sketch: 'cards', launch: () => openInternetExplorer() },
  [NOTEPAD_KEY]: { title: NOTEPAD_TITLE, icon: NOTEPAD_ICON, Body: () => <NotepadIcon />, size: { w: 2, h: 3 }, sketch: 'cards', launch: () => openNotepad() },
  [PROOFREAD_KEY]: { title: PROOFREAD_TITLE, icon: PROOFREAD_ICON, Body: () => <ProofreadIcon />, size: { w: 2, h: 3 }, sketch: 'cards', launch: () => openProofread() },
  [TERMINAL_KEY]: { title: TERMINAL_TITLE, icon: TERMINAL_ICON, Body: ({ s }) => <TerminalIcon cwd={s.cwd} />, size: { w: 2, h: 3 }, sketch: 'cards', launch: ({ s }) => openTerminal(s.cwd) },
  remote: { title: REMOTE_TITLE, icon: REMOTE_ICON, Body: RemoteIcon, size: { w: 2, h: 3 }, sketch: 'cards', launch: ({ s }) => openRemote(s) },
  chat: { title: 'チャット', icon: '💬', Body: Chat, size: { w: 18, h: ROWS - 6 }, sketch: 'chat' },
  outbox: { title: '送信したメッセージ', icon: '📤', Body: OutboxList, size: { w: 6, h: 10 }, sketch: 'list' },
  tasks: { title: 'タスク', icon: '✅', Body: TaskList, size: { w: 6, h: 8 }, sketch: 'list' },
  // 保存した配置が id で参照するので、shell と Monitor を並べるようにしても id は subagents のまま残す。
  subagents: { title: 'エージェント', icon: '🤖', Body: AgentList, size: { w: 6, h: 6 }, sketch: 'list' },
  docs: { title: '資料', icon: '📄', Body: SessionDocs, size: { w: 6, h: 10 }, sketch: 'list' },
  code: { title: 'コード', icon: '💻', Body: CodeWidget, size: { w: COLS, h: ROWS }, sketch: 'code' },
  [PR_WIDGET]: { title: PR_TITLE, icon: GITHUB_ICON, Body: PrWidget, size: { w: 12, h: ROWS - 6 }, sketch: 'text' },
  [JIRA_WIDGET]: { title: JIRA_VIEW_TITLE, icon: JIRA_ICON, Body: JiraWidget, size: { w: 12, h: ROWS - 6 }, sketch: 'text' },
  pr: { title: PR_REVIEW_TITLE, icon: '🔍', Body: PrReview, size: { w: 12, h: ROWS - 6 }, sketch: 'chat' },
  usage: { title: '使用量', icon: '📊', Body: UsageWidget, size: { w: COLS, h: ROWS }, sketch: 'chart' },
};

// 初期配置は 1 画面に収め、スクロールしなくても使えるようにする。チャット、コード、PR、PR コメント、JIRA、使用量と、右の一覧は、それぞれ 1 つの枠に重ねてタブで切り替える。
const DEFAULT_LAYOUT: Layout = [
  { id: 'head', x: 0, y: 0, w: 10, h: 6, tabs: [{ id: 'head' }] },
  { id: 'summary', x: 10, y: 0, w: 6, h: 6, tabs: [{ id: 'summary' }] },
  { id: 'diagram', x: 16, y: 0, w: 2, h: 3, tabs: [{ id: 'diagram' }], free: true },
  { id: 'remote', x: 16, y: 3, w: 2, h: 3, tabs: [{ id: 'remote' }], free: true },
  { id: 'main', x: 0, y: 6, w: 18, h: ROWS - 6, tabs: [{ id: 'chat' }, { id: 'code' }, { id: PR_WIDGET }, { id: 'pr' }, { id: JIRA_WIDGET }, { id: 'usage' }] },
  { id: 'side', x: 18, y: 0, w: 6, h: ROWS, tabs: [{ id: 'outbox' }, { id: 'tasks' }, { id: 'subagents' }, { id: 'docs' }], active: 'tasks' },
];

type DashboardProps = EditProps & { s: Session; tab: SessionTab; file?: string; snapshot: Snapshot };

export const SESSION_KEY = 'session-dashboard';

const useSessionData = (s: Session, snapshot: Snapshot, file?: string): SessionData => {
  // ツールの呼び出しだけでも発言が増えるので、会話ログが動くたびに取り直す。
  const chat = useFetch(() => api.chat(s.id).then((entries) => ({ id: s.id, entries })), [s.id, s.updatedAt]);
  // 開いた直後と別のセッションへ移った直後だけ砂時計にする。会話が動くたびの取り直しでは出さない。
  useWaitCursor(chat.loading && chat.data?.id !== s.id);
  const entries = chat.data?.entries ?? [];
  const [jump, setJump] = useState<Jump>();
  return {
    s,
    themes: snapshot.themes,
    file,
    outbox: snapshot.outbox,
    docs: snapshot.docs.filter((d) => d.sessionId === s.id),
    entries,
    asks: asksOf(snapshot.attention, s.id),
    jump,
    onJump: (index: number) => setJump({ index }),
    notion: !!snapshot.notion,
  };
};

/**
 * 配置はすべてのセッションで共通にし、ホームとは別に保存する。
 * コードや使用量の画面へのリンクで開いたときは、そのウィジェットを重ねた枠でそのタブを選び、枠までスクロールする。ファイルを開くときは、コードのウィジェットがそのファイルまで送る。
 */
const SessionDashboard = ({ s, tab, file, snapshot, editing, onEditDone }: DashboardProps) => {
  const data = useSessionData(s, snapshot, file);
  const focus = useMemo<WidgetFocus | undefined>(() => (tab === 'overview' ? undefined : { kind: tab }), [tab, file]);
  useEffect(() => {
    if (focus && !file) document.querySelector(`.widget[data-tabs~="${tab}"]`)?.scrollIntoView({ block: 'start' });
  }, [focus]);
  return (
    <Dashboard
      widgets={WIDGETS}
      defaults={DEFAULT_LAYOUT}
      storageKey={SESSION_KEY}
      scope={s.id}
      focus={focus}
      data={data}
      editing={editing}
      onEditDone={onEditDone}
    />
  );
};

export const SessionPage = ({ id, ...props }: Omit<DashboardProps, 's'> & { id: string }) => {
  const s = props.snapshot.sessions.find((x) => x.id === id);
  if (!s) {
    return (
      <div class="card panel">
        <Empty>セッションが見つかりません。</Empty>
        <a href={href.home()}>ホームへ戻る</a>
      </div>
    );
  }
  return <SessionDashboard s={s} {...props} />;
};

const SessionWindow = ({ f, s, snapshot }: { f: Float; s: Session; snapshot: Snapshot }) => (
  <FloatWindow f={f} widgets={WIDGETS} data={useSessionData(s, snapshot)} titleOf={(t) => `${t} - ${s.name}`} />
);

/** セッションが一覧から消えたウィンドウは出さない。保存は残すので、セッションが戻れば、また出す。 */
export const SessionFloat = ({ f, snapshot }: { f: Float; snapshot: Snapshot }) => {
  const s = snapshot.sessions.find((x) => x.id === f.scope);
  return s ? <SessionWindow f={f} s={s} snapshot={snapshot} /> : null;
};
