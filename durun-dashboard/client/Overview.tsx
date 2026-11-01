// セッション概要のダッシュボードに置く、概要、チャット、タスク、エージェントのウィジェット。
import type { ComponentChildren } from 'preact';
import { useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'preact/hooks';
import { type Attention, type BgTask, type Bookmark, type ChatEntry, type ObservedTask, type OutboxMessage, PR_URL, type RepoRef, type Session, type Snapshot, type Subagent, bookmarkKey } from '../shared/types.ts';
import { DELIVERY_TOAST, api, attempt } from './api.ts';
import { DesktopIcon } from './apps.tsx';
import { refSplitter } from './chatLinks.ts';
import type { Badges } from './completion.ts';
import { type Float, FloatFrame, dock, floatUp } from './Float.tsx';
import { WidgetPanel, showWidget } from './Dashboard.tsx';
import { fileLink, openDocFloat } from './Docs.tsx';
import { explain } from './Dolphin.tsx';
import { dateTime, fmtTokens, shortDateTime, truncate } from './format.ts';
import { NowContext, useCached, useFetch, useWaitCursor } from './hooks.ts';
import { useTermMarks } from './Glossary.tsx';
import { useLightMode } from './lightMode.ts';
import { type CodeLink, Markdown, type MarkText } from './Markdown.tsx';
import { Composer, MessageInput } from './Messaging.tsx';
import { ARROW_STEP } from './order.ts';
import { href } from './router.ts';
import { SESSION_KEY } from './SessionPage.tsx';
import { toast } from './toast.tsx';
import { Chip, Empty, Ext, Loading, StatusBadge, onSubmitKey } from './ui.tsx';

const TASK_LABEL: Record<ObservedTask['status'], string> = { pending: '未着手', in_progress: '作業中', completed: '完了' };

const TaskRow = ({ t }: { t: ObservedTask }) => (
  <li class={`task task-${t.status}`}>
    <span class="task-icon" role="img" aria-label={TASK_LABEL[t.status]} />
    <span class="task-text" title={t.subject}>{t.subject}</span>
  </li>
);

/** タスクがまだ無いときの説明。サーバーが判定するのは、アーカイブしていない、やり取りのあるセッションだけ。 */
const noTasks = (s: Session): string =>
  s.archived ? 'アーカイブしたセッションでは、タスクをまとめません。' : !(s.lastPrompt || s.lastMessage) ? 'まだやり取りがありません。' : 'タスクをまとめています…';

export const TaskList = ({ s }: { s: Session }) => {
  const tasks = s.observedTasks;
  const done = tasks?.filter((t) => t.status === 'completed').length;
  return (
    <WidgetPanel title="タスク" count={tasks?.length}>
      {!tasks ? <Empty>{noTasks(s)}</Empty> : tasks.length === 0 ? <Empty>タスクはありません。</Empty> : (
        <>
          <p class="help">{done} / {tasks.length} 完了</p>
          <ul class="tasks">{tasks.map((t, i) => <TaskRow key={i} t={t} />)}</ul>
        </>
      )}
    </WidgetPanel>
  );
};

/** 出力が増えるたびに取り直す。依頼を書き直して巻き戻す先はメインの会話なので、巻き戻しのボタンは出さない。 */
const SubagentChat = ({ sessionId, a }: { sessionId: string; a: Subagent }) => {
  const { data, error } = useFetch(() => api.subagentChat(sessionId, a.id), [sessionId, a.id, a.outputTokens]);
  const codeLink = useMemo(() => fileLink(sessionId), [sessionId]);
  if (error) return <Empty>会話を読み込めません: {error}</Empty>;
  if (!data) return <Loading kind="chat" />;
  if (data.length === 0) return <Empty>まだやり取りがありません。</Empty>;
  return (
    <div class="chat subagent-chat">
      {data.map((e, i) => <ChatBubble key={i} sessionId={sessionId} e={{ ...e, rewindTo: undefined }} codeLink={codeLink} focused={false} />)}
    </div>
  );
};

/** 押すと、そのサブエージェントの会話を下に開く。会話は開いている間だけ取る。 */
const SubagentRow = ({ sessionId, a }: { sessionId: string; a: Subagent }) => {
  const [open, setOpen] = useState(false);
  return (
    <li class="subagent">
      <details onToggle={(e) => setOpen(e.currentTarget.open)}>
        <summary class="subagent-row">
          <span class={`run ${a.running ? 'is-running' : ''}`}>
            <i class="dot" aria-hidden="true" />
            {a.running ? '実行中' : '完了'}
          </span>
          <span class="subagent-desc">{a.description}</span>
          <span class="subagent-meta">
            <Chip>{a.type}</Chip>
            <span title="出力トークン">出力 {fmtTokens(a.outputTokens)}</span>
          </span>
        </summary>
        {open && <SubagentChat sessionId={sessionId} a={a} />}
      </details>
    </li>
  );
};

const BG_STATUS: Record<BgTask['status'], string> = { running: '実行中', completed: '完了', failed: '失敗', killed: '停止', stopped: '停止' };
const BG_KIND: Record<BgTask['kind'], string> = { shell: 'shell', monitor: 'Monitor' };

/** 状態が変わるたびに取り直す。実行中の出力は、開き直すと最新になる。 */
const BgDetail = ({ sessionId, t }: { sessionId: string; t: BgTask }) => {
  const { data, error } = useFetch(() => api.bgOutput(sessionId, t.id), [sessionId, t.id, t.status]);
  return (
    <div class="subagent-chat">
      {t.startedAt && <p class="help">開始 {dateTime(t.startedAt)}</p>}
      <pre class="bg-command">{t.command}</pre>
      {t.summary && <p class="help">{t.summary}</p>}
      {error ? <Empty>出力を読み込めません: {error}</Empty>
        : !data ? <Loading />
        : data.output ? <pre class="bg-output">{data.output}</pre>
        : <Empty>出力はありません。</Empty>}
    </div>
  );
};

const BgRow = ({ sessionId, t }: { sessionId: string; t: BgTask }) => {
  const [open, setOpen] = useState(false);
  const running = t.status === 'running';
  return (
    <li class="subagent">
      <details onToggle={(e) => setOpen(e.currentTarget.open)}>
        <summary class="subagent-row">
          <span class={`run ${running ? 'is-running' : ''}`}>
            <i class="dot" aria-hidden="true" />
            {BG_STATUS[t.status]}
          </span>
          <span class="subagent-desc">{t.description || t.command.split('\n')[0]}</span>
          <span class="subagent-meta"><Chip>{BG_KIND[t.kind]}</Chip></span>
        </summary>
        {open && <BgDetail sessionId={sessionId} t={t} />}
      </details>
    </li>
  );
};

/** メインエージェントの状態。いま進めているタスクがあれば、それを添える。 */
const MainAgent = ({ s }: { s: Session }) => {
  const active = s.observedTasks?.find((t) => t.status === 'in_progress');
  return (
    <div class="subagent agent-main">
      <StatusBadge status={s.status} waitingFor={s.waitingFor} />
      <span class="subagent-desc">{active?.subject ?? s.name}</span>
      <span class="subagent-meta">
        {s.usage.model && <Chip>{s.usage.model}</Chip>}
        <span title="コンテキストの使用率">コンテキスト {Math.round(s.usage.contextPercent)}%</span>
      </span>
    </div>
  );
};

/** エージェントのウィジェットの 1 区分。count を渡した区分は、0 件なら empty を出す。 */
const AgentSection = ({ title, count, empty, children }: { title: string; count?: number; empty?: string; children: ComponentChildren }) => (
  <section class="agent-section" aria-label={title}>
    <h3 class="agent-section-title">
      {title}
      {count !== undefined && <span class="count-pill">{count}</span>}
    </h3>
    {count === 0 ? <p class="help">{empty}</p> : children}
  </section>
);

const runningFirst = <T,>(list: T[], running: (item: T) => boolean): T[] => list.toSorted((a, b) => Number(running(b)) - Number(running(a)));

/**
 * メインエージェント、サブエージェント、ツール（バックグラウンドの shell と Monitor）の 3 つに分けて並べる。
 * サブエージェントとツールは、実行中のものから並べる。見出しの件数には、メインエージェントを入れない。
 */
export const AgentList = ({ s }: { s: Session }) => {
  const subagents = runningFirst(s.subagents, (a) => a.running);
  const tools = runningFirst(s.background.toReversed(), (t) => t.status === 'running');
  return (
    <WidgetPanel title="エージェント" count={subagents.length + tools.length}>
      <AgentSection title="メインエージェント">
        <MainAgent s={s} />
      </AgentSection>
      <AgentSection title="サブエージェント" count={subagents.length} empty="サブエージェントはいません。">
        <ul class="subagents">{subagents.map((a) => <SubagentRow key={a.id} sessionId={s.id} a={a} />)}</ul>
      </AgentSection>
      <AgentSection title="ツール" count={tools.length} empty="バックグラウンドで動かしたツールはありません。">
        <ul class="subagents">{tools.map((t) => <BgRow key={t.id} sessionId={s.id} t={t} />)}</ul>
      </AgentSection>
    </WidgetPanel>
  );
};

export const DIAGRAM_ICON = '/diagram.svg';

// 図解を作っているセッション。ほかのセッションへ移って戻っても作り終えるまで砂時計を出すため、部品の外に持つ。
const making = new Set<string>();
const makingListeners = new Set<() => void>();

const setMaking = (id: string, on: boolean): void => {
  if (on) making.add(id);
  else making.delete(id);
  makingListeners.forEach((fn) => fn());
};

const useMaking = (id: string): boolean => {
  const [, rerender] = useState(0);
  useEffect(() => {
    const fn = () => rerender((n) => n + 1);
    makingListeners.add(fn);
    return () => {
      makingListeners.delete(fn);
    };
  }, []);
  return making.has(id);
};

const canDiagram = (s: Session): boolean => !!(s.lastPrompt || s.lastMessage);

/** セッションの作業の図解を作り、作り終えたら、図解のアイコンのウィンドウで開く。作っている間にもう一度呼ばれても作らない。 */
const makeDiagram = async (s: Session, note: string): Promise<void> => {
  if (making.has(s.id)) return;
  // アイコンを置いていない画面でスタートメニューから起動しても、作り始めたことがわかるようにする。
  toast('図解を作っています。数分かかります。');
  setMaking(s.id, true);
  const doc = await attempt('図解', () => api.diagram(s.id, note));
  setMaking(s.id, false);
  if (!doc) return;
  showWidget(SESSION_KEY, s.id, 'docs');
  openDocFloat(doc.id, DIAGRAM_ICON);
};

/** 図解の指示のウィンドウの保存キー。scope にセッションの id を入れ、タブの id にもこのキーを入れる。 */
export const DIAGRAM_PROMPT_KEY = 'diagram-prompt';
// 指示の欄とボタンがちょうど収まる大きさ（px）。
const PROMPT_SIZE = { w: 420, h: 220 };
const closePrompt = (id: string) => dock(DIAGRAM_PROMPT_KEY, id, DIAGRAM_PROMPT_KEY);

/** 図解を作る前に、指示を書くウィンドウを開く。作っている間ややり取りがない間は開かない。 */
export const launchDiagram = (s: Session): void => {
  if (making.has(s.id)) return;
  if (!canDiagram(s)) return toast('まだやり取りがないので、図解を作れません。', 'error');
  floatUp(DIAGRAM_PROMPT_KEY, s.id, { id: DIAGRAM_PROMPT_KEY }, PROMPT_SIZE);
};

const NOTES_KEY = 'diagram-notes';
const MAX_NOTES = 10;
// 長い指示でプルダウンの一覧が画面いっぱいに広がらないよう、先頭のこの文字数だけを出す。
const NOTE_LABEL_MAX = 40;

/** 最近の図解の指示を新しい順に返す。どのセッションの図解でも使い回せるよう、ブラウザに保存する。 */
const recentNotes = (): string[] => {
  try {
    return JSON.parse(localStorage.getItem(NOTES_KEY) ?? '[]') as string[];
  } catch {
    return [];
  }
};

const rememberNote = (note: string): void => {
  try {
    localStorage.setItem(NOTES_KEY, JSON.stringify([note, ...recentNotes().filter((n) => n !== note)].slice(0, MAX_NOTES)));
  } catch {
    // 保存できなくても、図解は作れる。
  }
};

const DiagramPrompt = ({ s }: { s: Session }) => {
  const [note, setNote] = useState('');
  const [recent] = useState(recentNotes);
  const ref = useRef<HTMLTextAreaElement>(null);
  useEffect(() => ref.current?.focus(), []);
  const start = () => {
    closePrompt(s.id);
    const text = note.trim();
    if (text) rememberNote(text);
    void makeDiagram(s, text);
  };
  // 選んだ指示に書き足してから始められるよう、入力欄にフォーカスを移す。
  const pick = (picked: string) => {
    setNote(picked);
    ref.current?.focus();
  };
  return (
    <div class="stack">
      <textarea
        ref={ref}
        class="input"
        rows={5}
        placeholder="図解の指示（任意）。例: データの流れを中心に描いて（Cmd/Ctrl + Enter で開始）"
        value={note}
        onInput={(e) => setNote(e.currentTarget.value)}
        onKeyDown={(e) => onSubmitKey(e, start)}
      />
      <div class="actions">
        {/* 入力欄を書き換えた後まで、前に選んだ指示が選ばれたままに見えないよう、選ぶたびに見出しの項目へ戻す。 */}
        {recent.length > 0 ? (
          <select class="input diagram-notes" aria-label="最近の指示" value="" onChange={(e) => pick(e.currentTarget.value)}>
            <option value="" disabled>最近の指示から選ぶ</option>
            {recent.map((n) => <option key={n} value={n}>{truncate(n, NOTE_LABEL_MAX)}</option>)}
          </select>
        ) : (
          <span class="spacer" />
        )}
        <button class="btn" onClick={() => closePrompt(s.id)}>キャンセル</button>
        <button class="btn btn-primary" onClick={start}>OK</button>
      </div>
    </div>
  );
};

/** セッションが一覧から消えたら、指示のウィンドウは出さない。 */
export const DiagramPromptFloat = ({ f, snapshot }: { f: Float; snapshot: Snapshot }) => {
  const s = snapshot.sessions.find((x) => x.id === f.scope);
  if (!s) return null;
  return (
    <FloatFrame f={f} title={`図解 - ${s.name}`} icon={DIAGRAM_ICON} controls={[{ kind: 'close', label: '閉じる', onClick: () => closePrompt(s.id) }]}>
      <DiagramPrompt s={s} />
    </FloatFrame>
  );
};

/**
 * デスクトップのアプリケーションアイコンのように、ダブルクリックか Enter で、図解の指示を書くウィンドウを開く。
 * 作るのに数分かかるので、作っている間はカーソルを砂時計にする。
 */
export const DiagramIcon = ({ s }: { s: Session }) => {
  const busy = useMaking(s.id);
  const ready = canDiagram(s);
  useWaitCursor(busy);
  const hint = ready ? 'ダブルクリックで、このセッションの作業を HTML で図解します' : 'まだやり取りがありません';
  return <DesktopIcon icon={DIAGRAM_ICON} title="図解" hint={hint} busy={busy} onLaunch={() => launchDiagram(s)} />;
};

export const REMOTE_TITLE = 'claude.ai/code で開く';
export const REMOTE_ICON = '/claude-open.svg';

/** セッションを claude.ai/code の新しいタブで開く。Remote Control でつないでいないセッションは開けない。 */
export const openRemote = (s: Session): void => {
  if (!s.remoteUrl) return toast('このセッションは claude.ai/code につないでいないので、開けません。', 'error');
  window.open(s.remoteUrl, '_blank', 'noopener');
};

export const RemoteIcon = ({ s }: { s: Session }) => (
  <DesktopIcon
    icon={REMOTE_ICON}
    title={REMOTE_TITLE}
    hint={s.remoteUrl ? 'ダブルクリックで、このセッションを claude.ai/code で開きます' : 'このセッションは claude.ai/code につないでいません'}
    onLaunch={() => openRemote(s)}
  />
);

/** やり取りが進むたびにまとめ直す。まとめ直す間は、前の概要を薄く出しておく。 */
export const Summary = ({ s }: { s: Session }) => {
  const ready = !!(s.lastPrompt || s.lastMessage);
  const progress = `${s.status} ${s.lastMessageAt} ${s.lastPrompt} ${s.tasks.map((t) => t.status).join()}`;
  const { data, loading, error } = useCached(() => (ready ? api.summary(s.id) : Promise.resolve(undefined)), [s.id, progress, ready]);
  return (
    <WidgetPanel title="概要">
      {data ? (
        <dl class={`summary ${loading || data.stale ? 'is-loading' : ''}`}>
          <dt>やっていること</dt>
          <dd>{data.doing}</dd>
          <dt>いま</dt>
          <dd>{data.now}</dd>
        </dl>
      ) : (
        <Empty>{!ready ? 'まだやり取りがありません。' : error ? `まとめられませんでした（${error}）` : 'まとめています…'}</Empty>
      )}
    </WidgetPanel>
  );
};

/**
 * 文の #番号のうち、セッションのリポジトリの新しい PR と Issue に番号が合うものを、マウスを載せると題名が出るリンクにする。ほかの文は mark で描く。
 * PR はイルカが吹き出しで説明するので、題名はイルカのいない軽量モードでだけ出す。
 * PR へのリンクは、押すとダッシュボードの中で開く。
 */
const useRefMarks = (s: Session, mark?: MarkText): MarkText => {
  const { data } = useCached((fresh) => api.repoRefs({ sessionId: s.id }, fresh), [s.id]);
  const light = useLightMode();
  return useMemo(() => {
    const split = refSplitter(data ?? []);
    const title = (r: RepoRef) => (r.kind === 'pr' && !light ? undefined : r.title);
    return (text: string) => split(text).map((p, i) => (p.ref ? <Ext key={i} href={p.ref.url} title={title(p.ref)}>{p.text}</Ext> : mark ? mark(p.text) : p.text));
  }, [data, mark, light]);
};

/** PR へのリンクにマウスを載せたら、イルカが吹き出しで PR を説明する。コメントなど PR の中の場所へのリンクは説明しない。 */
const explainPrLink = (e: MouseEvent) => {
  const a = e.target instanceof Element ? e.target.closest('a') : null;
  if (a && PR_URL.test(a.href)) explain({ pr: a.href, at: a });
};

const CHAT_PAGE = 100;
// 一番下からこの距離までなら、一番下を見ているとみなす。
const STICK_PX = 40;

/**
 * 過去の依頼を書き直して送る欄。確定すると、会話をその依頼の直前まで巻き戻してから送る。
 * 届くまで欄を残し、失敗したら書き直した文を消さずに残す。
 */
const PromptEditor = ({ sessionId, badges, e, onClose }: { sessionId: string; badges?: Badges; e: ChatEntry; onClose: () => void }) => {
  const [text, setText] = useState(e.text);
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => input.current?.focus(), []);
  const send = async () => {
    if (busy || !text.trim() || !e.rewindTo) return;
    setBusy(true);
    const res = await attempt('巻き戻して送信', () => api.rewind(sessionId, e.rewindTo ?? '', text));
    setBusy(false);
    if (!res) return;
    toast(DELIVERY_TOAST[res.delivery]);
    onClose();
  };
  return (
    <div class="chat-edit">
      <MessageInput sessionId={sessionId} badges={badges} value={text} setValue={setText} placeholder="書き直した依頼（Cmd/Ctrl + Enter で巻き戻して送信）" onSubmit={send} inputRef={input} />
      <div class="actions">
        <button class="btn" onClick={onClose}>キャンセル</button>
        <button class="btn btn-primary" disabled={busy || !text.trim()} title="この依頼の直前まで会話を巻き戻して送ります" onClick={send}>巻き戻して送信</button>
      </div>
    </div>
  );
};

/** 押すたびに、表示している状態の逆を送る。 */
const BookmarkButton = ({ sessionId, mark, on }: { sessionId: string; mark: Pick<Bookmark, 'kind' | 'at' | 'text'>; on: boolean }) => (
  <button
    class="chat-bookmark"
    aria-pressed={on}
    aria-label="ブックマーク"
    title={on ? 'ブックマークから外す' : 'ブックマークする。アプリ「ブックマーク」から、この発言へ戻れます'}
    onClick={() => attempt('ブックマークの切り替え', () => api.bookmark(sessionId, mark, !on))}
  >
    {on ? '★' : '☆'}
  </button>
);

/** ブックマークから移れるのはセッションのチャットの発言だけなので、bookmarked を渡さないサブエージェントの会話には、ブックマークのボタンを出さない。 */
type ChatBubbleProps = { sessionId: string; badges?: Badges; e: ChatEntry; codeLink: CodeLink; markText?: MarkText; focused: boolean; bookmarked?: boolean };

const ChatBubble = ({ sessionId, badges, e, codeLink, markText, focused, bookmarked }: ChatBubbleProps) => {
  const [editing, setEditing] = useState(false);
  const now = useContext(NowContext);
  const time = e.at ? <time title={dateTime(e.at)}>{shortDateTime(e.at, now)}</time> : null;
  // ツールの呼び出しは数が多く発言を埋もれさせるので、名前と要点の 1 行だけ見せて入力は畳んでおく。
  if (e.kind === 'tool') return (
    <details class="chat-tool">
      <summary>{e.text}</summary>
      <pre>{e.detail}</pre>
    </details>
  );
  if (e.kind === 'prompt' && editing) return <PromptEditor sessionId={sessionId} badges={badges} e={e} onClose={() => setEditing(false)} />;
  // 発言は種類と時刻で見分けるので、時刻の無い発言はブックマークできない。
  const star = bookmarked !== undefined && e.at !== undefined && <BookmarkButton sessionId={sessionId} mark={{ kind: e.kind, at: e.at, text: e.text }} on={bookmarked} />;
  // 依頼の ☆ は、短い依頼の行を高くしないよう、時刻の列に積まずに ✎ と並べる。
  return e.kind === 'prompt' ? (
    <div class="chat-row is-human">
      {e.rewindTo && <button class="chat-rewind" title="書き直して、ここまで巻き戻して送る" aria-label="依頼を書き直す" onClick={() => setEditing(true)}>✎</button>}
      {star}
      {/* チャットは会話ログから作るので、並んでいる依頼はどれも Claude が受け取って読んだもの。 */}
      <span class="chat-meta">既読{time}</span>
      <p class={`chat-human plain${focused ? ' is-focused' : ''}`}>{markText ? markText(e.text) : e.text}</p>
    </div>
  ) : (
    <div class="chat-row">
      <span class="chat-meta">{star}{time}</span>
      <div class={`card chat-claude${focused ? ' is-focused' : ''}`}><Markdown text={e.text} codeLink={codeLink} markText={markText} /></div>
    </div>
  );
};

/** Claude がまだ読んでいない、送ったメッセージ。既読を付けず、届けられなかったものには未配信と添える。 */
const UnreadBubble = ({ m }: { m: OutboxMessage }) => {
  const now = useContext(NowContext);
  return (
    <div class="chat-row is-human">
      <span class="chat-meta">{!m.deliveredAt && '未配信'}<time title={dateTime(m.createdAt)}>{shortDateTime(m.createdAt, now)}</time></span>
      <p class="chat-human plain">{m.text}</p>
    </div>
  );
};

/** 送信したメッセージとブックマークから移る先の発言。同じ発言へもう一度移れるよう、押すたびに作り直す。 */
export type Jump = { index: number };

/**
 * 一般的な AI チャットと同じく、古い順に並べて一番下から見せ、上端に近づいたら前の発言を読み込んでさかのぼる。
 * 新しい発言が来たら、一番下を見ているときだけ送る。読み返している位置は動かさない。
 * entries は会話ログを読み終えるまで無いので、その間は吹き出しの形の帯を出す。読めなければ chatError を出す。
 * unread は Claude がまだ読んでいない送ったメッセージで、会話の後ろに並べる。
 * asks は Claude が人間の操作を待っている質問、コマンドの実行の依頼、権限の確認。質問と依頼は送信欄の代わりに、権限の確認は送信欄の上に出す。
 */
export const Chat = ({ s, entries, chatError, unread, asks, jump }: { s: Session; entries?: ChatEntry[]; chatError?: string; unread: OutboxMessage[]; asks: Attention[]; jump?: Jump }) => {
  const list = entries ?? [];
  // 表示の始まりは最初に届いたときに決めて固定し、発言が増えても上の発言を押し出さない。
  const [start, setStart] = useState<number>();
  // 送信したメッセージから移るときは、移る先の発言まで表示を広げる。
  const first = Math.min(start ?? Math.max(0, list.length - CHAT_PAGE), jump?.index ?? list.length, list.length);
  const box = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const fromBottom = useRef(0);
  // 前の依頼へ送るために前の発言を読み込んだときに立て、読み込み終えたら送り直す。
  const retry = useRef(false);
  useEffect(() => {
    if (start === undefined && list.length) setStart(first);
  }, [list.length]);
  useLayoutEffect(() => {
    if (box.current && stick.current) box.current.scrollTop = box.current.scrollHeight;
  }, [list.length, unread.length]);
  // 前を読み込んだら、読んでいた発言が同じ位置に残るよう、下からの距離を保つ。
  useLayoutEffect(() => {
    if (box.current) box.current.scrollTop = box.current.scrollHeight - fromBottom.current;
    if (!retry.current) return;
    retry.current = false;
    toPrompt(-1);
  }, [first]);
  useLayoutEffect(() => {
    if (jump) box.current?.querySelector('.is-focused')?.scrollIntoView({ block: 'center' });
  }, [jump]);
  // 上端まで 1 画面を切ったら前の発言を読み込み、上端で止まらずにさかのぼれるようにする。
  const nearTop = (el: HTMLElement, scrollTop: number) => first > 0 && scrollTop < el.clientHeight;
  const loadBefore = (index: number) => setStart(Math.max(0, index - CHAT_PAGE));
  const onScroll = () => {
    const el = box.current;
    if (!el) return;
    fromBottom.current = el.scrollHeight - el.scrollTop;
    stick.current = fromBottom.current - el.clientHeight < STICK_PX;
    if (nearTop(el, el.scrollTop)) loadBefore(first);
  };
  // 表示中の位置より上（dir = -1）か下（dir = 1）で、いちばん近い依頼を欄の上端へ送る。最後の依頼より下へ進むときは、最新の発言が見える一番下まで送る。
  const toPrompt = (dir: number) => {
    const el = box.current;
    if (!el) return;
    const top = el.getBoundingClientRect().top;
    const offsets = [...el.querySelectorAll('.chat-row.is-human')].map((r) => r.getBoundingClientRect().top - top);
    const to = dir < 0 ? offsets.findLast((o) => o < -1) : offsets.find((o) => o > 1) ?? el.scrollHeight;
    // 前の依頼をまだ読み込んでいないときや、送った先で前の発言の読み込みが始まるときは、先に読み込んでから送り直す。
    // 送っている途中で読み込むと、読んでいた位置を保つスクロールで送るのが止まる。
    if (dir < 0 && (to === undefined || nearTop(el, el.scrollTop + to))) {
      const prev = to === undefined ? list.findLastIndex((e, i) => i < first && e.kind === 'prompt') : first;
      if (prev < 0) return;
      retry.current = true;
      loadBefore(prev);
      return;
    }
    if (to !== undefined) el.scrollBy({ top: to, behavior: 'smooth' });
  };
  // チャットを押してから上下の矢印キーを押しても、↑↓ のボタンと同じく依頼へ移る。押したチャットにキーが届くよう、tabIndex でフォーカスできるようにする。
  // 依頼を書き直す欄では文字のカーソルを動かし、Cmd + ↑ のような修飾キーつきの操作はブラウザのスクロールに任せる。
  const onKeyDown = (e: KeyboardEvent) => {
    const dir = ARROW_STEP[e.key];
    if (!dir || e.metaKey || e.ctrlKey || e.altKey || e.shiftKey || (e.target as Element).closest('textarea')) return;
    e.preventDefault();
    toPrompt(dir);
  };
  const codeLink = useMemo(() => fileLink(s.id), [s.id]);
  const markText = useRefMarks(s, useTermMarks(s));
  const marked = new Set(s.bookmarks?.map(bookmarkKey));
  return (
    <>
      <h2 class="section-title">チャット</h2>
      <div class="widget-body chat" ref={box} tabIndex={0} onScroll={onScroll} onKeyDown={onKeyDown} onMouseOver={explainPrLink}>
        {!entries ? (chatError ? <Empty>会話を読み込めません: {chatError}</Empty> : <Loading kind="chat" />)
          : list.length + unread.length === 0 ? <Empty>まだやり取りがありません。</Empty> : (
          <>
            {first > 0 && <button class="btn btn-sm chat-more" onClick={() => loadBefore(first)}>前の {Math.min(CHAT_PAGE, first)} 件を表示</button>}
            {list.slice(first).map((e, i) => <ChatBubble key={first + i} sessionId={s.id} badges={s} e={e} codeLink={codeLink} markText={markText} focused={first + i === jump?.index} bookmarked={marked.has(bookmarkKey(e))} />)}
            {unread.map((m) => <UnreadBubble key={m.id} m={m} />)}
            <div class="chat-nav">
              <button class="btn btn-sm" title="前の依頼へ" aria-label="前の依頼へ" onClick={() => toPrompt(-1)}>↑</button>
              <button class="btn btn-sm" title="次の依頼へ" aria-label="次の依頼へ" onClick={() => toPrompt(1)}>↓</button>
            </div>
          </>
        )}
      </div>
      <Composer s={s} asks={asks} />
    </>
  );
};
