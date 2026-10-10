// セッション概要のダッシュボードに置く、概要、チャット、タスク、エージェントのウィジェット。
import type { ComponentChildren } from 'preact';
import { useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'preact/hooks';
import type { Attention, BgTask, ChatEntry, FileWhere, Session, Subagent, TaskItem } from '../shared/types.ts';
import { DELIVERY_TOAST, api, attempt } from './api.ts';
import { DesktopIcon } from './apps.tsx';
import { WidgetPanel, showWidget } from './Dashboard.tsx';
import { openDocFloat, openFileFloat, plainClick, whereOf } from './Docs.tsx';
import { dateTime, fmtTokens, shortDateTime } from './format.ts';
import { isFileRef } from './fileRef.ts';
import { NowContext, useFetch } from './hooks.ts';
import { type CodeLink, Markdown } from './Markdown.tsx';
import { Composer, MessageInput } from './Messaging.tsx';
import { href } from './router.ts';
import { SESSION_KEY } from './SessionPage.tsx';
import { toast } from './toast.tsx';
import { Chip, Empty, StatusBadge } from './ui.tsx';

const TASK_LABEL: Record<TaskItem['status'], string> = { pending: '未着手', in_progress: '作業中', completed: '完了' };

const TaskRow = ({ t }: { t: TaskItem }) => {
  const active = t.status === 'in_progress';
  const blocked = t.status !== 'completed' && t.blockedBy.length > 0;
  return (
    <li class={`task task-${t.status}`}>
      <span class="task-icon" role="img" aria-label={TASK_LABEL[t.status]} />
      <span class="task-text" title={t.subject}>{active ? t.activeForm ?? t.subject : t.subject}</span>
      {blocked && <Chip title={`先行タスク ${t.blockedBy.join(', ')} の完了待ち`}>待ち {t.blockedBy.length}</Chip>}
    </li>
  );
};

export const TaskList = ({ s: { tasks } }: { s: Session }) => {
  const done = tasks.filter((t) => t.status === 'completed').length;
  return (
    <WidgetPanel title="タスク" count={tasks.length}>
      {tasks.length === 0 ? <Empty>タスクはありません。</Empty> : (
        <>
          <p class="help">{done} / {tasks.length} 完了</p>
          <ul class="tasks">{tasks.map((t) => <TaskRow key={t.id} t={t} />)}</ul>
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
  if (!data) return <Empty>読み込み中です。</Empty>;
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
        : !data ? <Empty>読み込み中です。</Empty>
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
  const active = s.tasks.find((t) => t.status === 'in_progress');
  return (
    <div class="subagent agent-main">
      <StatusBadge status={s.status} />
      <span class="subagent-desc">{active ? active.activeForm ?? active.subject : s.name}</span>
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
export const makeDiagram = async (s: Session): Promise<void> => {
  if (making.has(s.id)) return;
  if (!canDiagram(s)) return toast('まだやり取りがないので、図解を作れません。', 'error');
  // アイコンを置いていない画面でスタートメニューから起動しても、作り始めたことがわかるようにする。
  toast('図解を作っています。数分かかります。');
  setMaking(s.id, true);
  const doc = await attempt('図解', () => api.diagram(s.id));
  setMaking(s.id, false);
  if (!doc) return;
  showWidget(SESSION_KEY, s.id, 'docs');
  openDocFloat(doc.id, DIAGRAM_ICON);
};

/**
 * デスクトップのアプリケーションアイコンのように、ダブルクリックか Enter で図解を作る。
 * 作るのに数分かかるので、作っている間はカーソルを砂時計にする。
 */
export const DiagramIcon = ({ s }: { s: Session }) => {
  const busy = useMaking(s.id);
  const ready = canDiagram(s);
  // そのセッションを開いている間は、画面のどこでもカーソルを砂時計にする。
  useEffect(() => {
    if (!busy) return;
    document.documentElement.classList.add('is-waiting');
    return () => document.documentElement.classList.remove('is-waiting');
  }, [busy]);
  const hint = ready ? 'ダブルクリックで、このセッションの作業を HTML で図解します' : 'まだやり取りがありません';
  return <DesktopIcon icon={DIAGRAM_ICON} title="図解" hint={hint} busy={busy} onLaunch={() => void makeDiagram(s)} />;
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
  const { data, loading, error } = useFetch(() => (ready ? api.summary(s.id) : Promise.resolve(undefined)), [s.id, progress, ready]);
  return (
    <WidgetPanel title="概要">
      {data ? (
        <dl class={`summary ${loading ? 'is-loading' : ''}`}>
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
 * ファイルを指すコードは、開けると分かったものだけをリンクにする。リポジトリにあるファイルはコードの画面で開き、リポジトリの外のファイルと、画像や動画と Markdown はウィンドウに浮かべる。
 * 修飾キー付きのクリックは、ブラウザの既定の動きに任せる。
 */
const fileLink = (id: string): CodeLink => ({ text }) => {
  const [where, setWhere] = useState<FileWhere>();
  useEffect(() => {
    let alive = true;
    if (isFileRef(text)) void whereOf(id, text).then((w) => alive && setWhere(w));
    return () => {
      alive = false;
    };
  }, [text]);
  if (!where) return <code>{text}</code>;
  const onClick = (e: MouseEvent) => {
    if (where === 'repo' || !plainClick(e)) return;
    e.preventDefault();
    openFileFloat(id, text);
  };
  return <a href={href.file(id, text)} onClick={onClick} data-attach={text}><code>{text}</code></a>;
};

const CHAT_PAGE = 100;
// 一番下からこの距離までなら、一番下を見ているとみなす。
const STICK_PX = 40;

/**
 * 過去の依頼を書き直して送る欄。確定すると、会話をその依頼の直前まで巻き戻してから送る。
 * 届くまで欄を残し、失敗したら書き直した文を消さずに残す。
 */
const PromptEditor = ({ sessionId, e, onClose }: { sessionId: string; e: ChatEntry; onClose: () => void }) => {
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
      <MessageInput sessionId={sessionId} value={text} setValue={setText} placeholder="書き直した依頼（Cmd/Ctrl + Enter で巻き戻して送信）" onSubmit={send} inputRef={input} />
      <div class="actions">
        <button class="btn" onClick={onClose}>キャンセル</button>
        <button class="btn btn-primary" disabled={busy || !text.trim()} title="この依頼の直前まで会話を巻き戻して送ります" onClick={send}>巻き戻して送信</button>
      </div>
    </div>
  );
};

type ChatBubbleProps = { sessionId: string; e: ChatEntry; codeLink: CodeLink; focused: boolean };

const ChatBubble = ({ sessionId, e, codeLink, focused }: ChatBubbleProps) => {
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
  if (e.kind === 'prompt' && editing) return <PromptEditor sessionId={sessionId} e={e} onClose={() => setEditing(false)} />;
  return e.kind === 'prompt' ? (
    <div class="chat-row is-human">
      {e.rewindTo && <button class="chat-rewind" title="書き直して、ここまで巻き戻して送る" aria-label="依頼を書き直す" onClick={() => setEditing(true)}>✎</button>}
      {/* チャットは会話ログから作るので、並んでいる依頼はどれも Claude が受け取って読んだもの。 */}
      <span class="chat-meta">既読{time}</span>
      <p class={`chat-human plain${focused ? ' is-focused' : ''}`}>{e.text}</p>
    </div>
  ) : (
    <div class="chat-row">
      <span class="chat-meta">{time}</span>
      <div class="card chat-claude"><Markdown text={e.text} codeLink={codeLink} /></div>
    </div>
  );
};

/** 送信したメッセージから移る先の発言。同じ発言へもう一度移れるよう、押すたびに作り直す。 */
export type Jump = { index: number };

/**
 * 一般的な AI チャットと同じく、古い順に並べて一番下から見せ、上の「前の n 件」でさかのぼる。
 * 新しい発言が来たら、一番下を見ているときだけ送る。読み返している位置は動かさない。
 * asks は Claude が答えを待っている質問。送信欄の代わりに出す。
 */
export const Chat = ({ s, entries, asks, jump }: { s: Session; entries: ChatEntry[]; asks: Attention[]; jump?: Jump }) => {
  // 表示の始まりは最初に届いたときに決めて固定し、発言が増えても上の発言を押し出さない。
  const [start, setStart] = useState<number>();
  // 送信したメッセージから移るときは、移る先の発言まで表示を広げる。
  const first = Math.min(start ?? Math.max(0, entries.length - CHAT_PAGE), jump?.index ?? entries.length, entries.length);
  const box = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const fromBottom = useRef(0);
  useEffect(() => {
    if (start === undefined && entries.length) setStart(first);
  }, [entries.length]);
  useLayoutEffect(() => {
    if (box.current && stick.current) box.current.scrollTop = box.current.scrollHeight;
  }, [entries.length]);
  // 前を読み込んだら、読んでいた発言が同じ位置に残るよう、下からの距離を保つ。
  useLayoutEffect(() => {
    if (box.current && !stick.current) box.current.scrollTop = box.current.scrollHeight - fromBottom.current;
  }, [first]);
  useLayoutEffect(() => {
    if (jump) box.current?.querySelector('.is-focused')?.scrollIntoView({ block: 'center' });
  }, [jump]);
  const onScroll = () => {
    const el = box.current;
    if (!el) return;
    fromBottom.current = el.scrollHeight - el.scrollTop;
    stick.current = fromBottom.current - el.clientHeight < STICK_PX;
  };
  // 表示中の位置より上（dir = -1）か下（dir = 1）で、いちばん近い依頼を欄の上端へ送る。
  const toPrompt = (dir: 1 | -1) => {
    const el = box.current;
    if (!el) return;
    const top = el.getBoundingClientRect().top;
    const offsets = [...el.querySelectorAll('.chat-row.is-human')].map((r) => r.getBoundingClientRect().top - top);
    const to = dir < 0 ? offsets.findLast((o) => o < -1) : offsets.find((o) => o > 1);
    if (to !== undefined) el.scrollBy({ top: to, behavior: 'smooth' });
  };
  const codeLink = useMemo(() => fileLink(s.id), [s.id]);
  return (
    <>
      <h2 class="section-title">チャット</h2>
      <div class="widget-body chat" ref={box} onScroll={onScroll}>
        {entries.length === 0 ? <Empty>まだやり取りがありません。</Empty> : (
          <>
            {first > 0 && <button class="btn btn-sm chat-more" onClick={() => setStart(Math.max(0, first - CHAT_PAGE))}>前の {Math.min(CHAT_PAGE, first)} 件を表示</button>}
            {entries.slice(first).map((e, i) => <ChatBubble key={first + i} sessionId={s.id} e={e} codeLink={codeLink} focused={first + i === jump?.index} />)}
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
