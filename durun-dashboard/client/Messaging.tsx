import type { RefObject } from 'preact';
import { useId, useLayoutEffect, useRef, useState } from 'preact/hooks';
import { EFFORT_LEVELS, MODELS, type Attention, type ChatEntry, type OutboxMessage, type QueuedMessage, type Session, type SessionStatus } from '../shared/types.ts';
import { DELIVERY_TOAST, api, attempt } from './api.ts';
import { insertAt, useAttachMode } from './attach.ts';
import { type Candidate, type Typed, candidates, typedAt } from './completion.ts';
import { WidgetPanel } from './Dashboard.tsx';
import { dateTime } from './format.ts';
import { useCached, useFetch } from './hooks.ts';
import { AttentionCard } from './Inbox.tsx';
import { toast } from './toast.tsx';
import { href, currentSessionId } from './router.ts';
import { Choice, ContextPie, Empty, Time, copyText, onSubmitKey } from './ui.tsx';

const STATUS_HELP: Record<SessionStatus, string> = {
  busy: '作業中です。ツール実行の合間に Claude が読みます。',
  idle: '入力待ちです。送信すると、Claude がすぐに読み始めます。',
  done: '完了しています。送信するとバックグラウンドで再開します。',
  interrupted: '中断しています。送信するとバックグラウンドで再開します。',
  ended: '終了しています。送信するとバックグラウンドで再開します。',
};

const VIA_LABEL: Record<NonNullable<OutboxMessage['via']>, string> = {
  socket: 'ソケット経由',
  hook: 'フック経由',
  resume: 'セッション再開',
  mcp: 'MCP 経由',
  manual: '手動',
};

const DELIVERED_LIMIT = 20;

/** 作業中は発言が次々に変わるので、Claude が止まってから作る。 */
const ReplySuggestions = ({ s, onPick }: { s: Session; onPick: (reply: string, replies: string[]) => void }) => {
  const ready = s.status !== 'busy' && !!s.lastMessage;
  const { data: replies = [], loading, error } = useFetch(() => (ready ? api.suggestions(s.id) : Promise.resolve([])), [s.id, s.lastMessageAt, ready]);
  if (!ready) return null;
  if (loading) return <p class="help">返答の候補を作っています…</p>;
  if (error) return <p class="help">返答の候補を作れませんでした（{error}）</p>;
  return (
    <div class="actions reply-options">
      {replies.map((r) => (
        <button key={r} class="btn reply-option" title="クリックすると入力欄に入ります" onClick={() => onPick(r, replies)}>{r}</button>
      ))}
    </div>
  );
};

const CARET_KEYS = new Set(['ArrowLeft', 'ArrowRight', 'Home', 'End']);
const COMPLETION_ID = 'completion-menu';

type CompletionMenuState = { items: Candidate[]; active: number };

/**
 * / か # から始まる語を打つと、候補を出す。/ は Claude Code がこのセッションに見せた skills で、まだセッションが無ければ出さない。
 * # はセッションか cwd のリポジトリの PR と Issue で、# の語を打っている間だけ読む。
 */
const useCompletion = (sessionId: string | undefined, cwd: string | undefined, input: RefObject<HTMLTextAreaElement>, setText: (text: string) => void) => {
  const [typed, setTyped] = useState<Typed>();
  const [active, setActive] = useState(0);
  const { data: skills = [] } = useFetch(() => (sessionId ? api.skills(sessionId) : Promise.resolve([])), [sessionId]);
  const wantsRefs = typed?.mark === '#' && !!(sessionId || cwd);
  const { data: refs = [] } = useCached((fresh) => (wantsRefs ? api.repoRefs({ sessionId, cwd }, fresh) : Promise.resolve([])), [sessionId, cwd, wantsRefs]);
  const items = typed ? candidates(typed, skills, refs) : [];
  const menu: CompletionMenuState | undefined = items.length ? { items, active: Math.min(active, items.length - 1) } : undefined;
  const update = () => {
    const el = input.current;
    setTyped(el ? typedAt(el.value.slice(0, el.selectionStart)) : undefined);
    setActive(0);
  };
  const close = () => setTyped(undefined);
  // 打ちかけの語を選んだ候補に置き換え、続けて書けるよう空白を足す。
  const accept = (picked: Candidate) => {
    const el = input.current;
    if (!el) return;
    const head = el.value.slice(0, el.selectionStart).replace(/[/#]\S*$/, `${picked.label} `);
    const next = head + el.value.slice(el.selectionEnd);
    el.value = next;
    el.setSelectionRange(head.length, head.length);
    setText(next);
    close();
  };
  /** 候補を出している間は、上下で選び、Enter か Tab で入れ、Esc で閉じる。扱ったキーなら true を返す。 */
  const onKeyDown = (e: KeyboardEvent): boolean => {
    if (!menu || e.isComposing || e.metaKey || e.ctrlKey) return false;
    const n = menu.items.length;
    const picked = menu.items[menu.active];
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') setActive((menu.active + (e.key === 'ArrowDown' ? 1 : n - 1)) % n);
    else if ((e.key === 'Enter' || e.key === 'Tab') && picked) accept(picked);
    else if (e.key === 'Escape') close();
    else return false;
    e.preventDefault();
    return true;
  };
  const onKeyUp = (e: KeyboardEvent) => CARET_KEYS.has(e.key) && update();
  return { menu, update, accept, onKeyDown, onKeyUp, close };
};

const CompletionMenu = ({ menu, onPick }: { menu: CompletionMenuState; onPick: (picked: Candidate) => void }) => {
  const list = useRef<HTMLUListElement>(null);
  useLayoutEffect(() => list.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' }), [menu.active]);
  return (
    <ul id={COMPLETION_ID} class="completion-menu" role="listbox" aria-label="入力の候補" ref={list}>
      {menu.items.map((c, i) => (
        // 押下でフォーカスが入力欄から外れると候補が閉じるので、押下の時点で入れる。
        <li
          key={c.label}
          id={`completion-${i}`}
          class="completion-option"
          role="option"
          aria-selected={i === menu.active}
          title={c.desc}
          onMouseDown={(e) => {
            e.preventDefault();
            onPick(c);
          }}
        >
          <span class="completion-label">{c.label}</span>
          <span class="completion-desc">{c.desc}</span>
        </li>
      ))}
    </ul>
  );
};

type MessageInputProps = {
  sessionId?: string;
  /** セッションが無いときに、# で PR と Issue を探すリポジトリのディレクトリ。 */
  cwd?: string;
  value: string;
  setValue: (value: string) => void;
  placeholder: string;
  onSubmit: () => void;
  inputRef?: RefObject<HTMLTextAreaElement>;
};

/** セッションへのメッセージと、新規セッションの最初の依頼とで使う入力欄。skills と PR・Issue の補完と「なんでも添付」を持つ。 */
export const MessageInput = ({ sessionId, cwd, value, setValue, placeholder, onSubmit, inputRef }: MessageInputProps) => {
  const own = useRef<HTMLTextAreaElement>(null);
  const input = inputRef ?? own;
  const completion = useCompletion(sessionId, cwd, input, setValue);
  // 入力欄の外をクリックしてもカーソル位置は残るので、そこへ入れる。
  const attach = useAttachMode((piece) => {
    const el = input.current;
    if (!el) return;
    const next = insertAt(el.value, el.selectionEnd, piece);
    el.value = next.value;
    el.setSelectionRange(next.caret, next.caret);
    setValue(next.value);
  });
  return (
    <div class="composer-field">
      <textarea
        ref={input}
        class="input"
        rows={3}
        placeholder={placeholder}
        aria-autocomplete="list"
        aria-controls={completion.menu ? COMPLETION_ID : undefined}
        aria-activedescendant={completion.menu ? `completion-${completion.menu.active}` : undefined}
        value={value}
        onInput={(e) => {
          setValue(e.currentTarget.value);
          completion.update();
        }}
        onKeyDown={(e) => completion.onKeyDown(e) || onSubmitKey(e, onSubmit)}
        onKeyUp={completion.onKeyUp}
        onClick={completion.update}
        onBlur={completion.close}
      />
      <button
        type="button"
        class="attach-toggle"
        aria-pressed={attach.on}
        title={attach.on ? '添付モードを終える（Esc）' : 'なんでも添付: 画面上の URL・ファイルパス・資料・バッジをクリックすると、メッセージに入れます'}
        aria-label="なんでも添付"
        onClick={attach.toggle}
      >
        +
      </button>
      {completion.menu && <CompletionMenu menu={completion.menu} onPick={completion.accept} />}
    </div>
  );
};

type SendMode = 'send' | 'queue';
const SEND_MODE_KEY = 'send-mode';
const QUEUE_HELP = 'Claude が入力待ちになるまで保留し、質問していなければ古い順に 1 つずつ送ります';

/** 選んだ送り方はブラウザに覚え、次に開いた送信欄でも使う。 */
const useSendMode = (): [SendMode, (mode: SendMode) => void] => {
  const [mode, setMode] = useState<SendMode>(() => (localStorage.getItem(SEND_MODE_KEY) === 'queue' ? 'queue' : 'send'));
  const choose = (next: SendMode) => {
    localStorage.setItem(SEND_MODE_KEY, next);
    setMode(next);
  };
  return [mode, choose];
};

/** 送信のボタンの右の ▾ で、すぐに送る「送信」と、保留する「キュー」を切り替える。 */
const SendModeMenu = ({ mode, onChange }: { mode: SendMode; onChange: (mode: SendMode) => void }) => {
  const id = useId();
  const item = (value: SendMode, label: string, help: string) => (
    <button class="send-mode-item" role="menuitemradio" aria-checked={mode === value} popovertarget={id} popovertargetaction="hide" onClick={() => onChange(value)}>
      {label}<span class="muted">{help}</span>
    </button>
  );
  return (
    <>
      <button class="btn btn-primary send-mode-toggle" popovertarget={id} aria-haspopup="menu" aria-label="送り方を選ぶ" title="送り方を選ぶ">▾</button>
      <div id={id} class="send-mode-menu" popover role="menu">
        {item('send', '送信', 'すぐに届けます')}
        {item('queue', 'キュー', QUEUE_HELP)}
      </div>
    </>
  );
};

const SWITCH_HELP = '選び直して送ると、会話を引き継いだ複製を選んだ設定で起動し、元のセッションはアーカイブします';
const FORK_HELP = '会話を引き継いだ別のセッションを選んでいるモデルと effort で起動してメッセージを渡し、その画面へ移ります。元のセッションは残ります';
const NO_SWITCH_HELP = '作業中のセッションと端末で開いているセッションは、ここからは変えられません';

/**
 * 止めて再開できるセッションだけ、モデルと effort を変えられる。
 * 端末で開いているセッションは、入力待ちでもダッシュボードからは止められない。
 */
const canSwitch = (s: Session): boolean => s.status !== 'busy' && (s.status !== 'idle' || s.kind === 'bg');

/**
 * 今のモデルと effort を選択肢で見せ、選び直した値を持つ。
 * 選択肢に無いモデルと、分からない effort は、空の選択肢に今の値を出す。
 */
const useModelChoice = (s: Session) => {
  const current = { model: MODELS.find((m) => s.usage.model?.includes(m)) ?? '', effort: EFFORT_LEVELS.find((e) => e === s.usage.effort) ?? '' };
  const [picked, setPicked] = useState<{ model?: string; effort?: string }>({});
  const model = picked.model ?? current.model;
  const effort = picked.effort ?? current.effort;
  const enabled = canSwitch(s);
  const title = enabled ? SWITCH_HELP : NO_SWITCH_HELP;
  return {
    /** 起動に渡す値。選び直していないモデルは、今の正式な名前で引き継ぐ。 */
    model: model || (s.usage.model ?? ''),
    effort,
    changed: enabled && (model !== current.model || effort !== current.effort),
    reset: () => setPicked({}),
    fields: (
      <>
        <Choice label="モデル" value={model} options={MODELS} empty={!current.model && (s.usage.model ?? '不明')} disabled={!enabled} title={title} onChange={(v) => setPicked((p) => ({ ...p, model: v }))} />
        <Choice label="effort" value={effort} options={EFFORT_LEVELS} empty={!current.effort && (s.usage.effort ?? '不明')} disabled={!enabled} title={title} onChange={(v) => setPicked((p) => ({ ...p, effort: v }))} />
      </>
    ),
  };
};

/** 返答の候補と入力欄。 */
const MessageForm = ({ s }: { s: Session }) => {
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLTextAreaElement>(null);
  const choice = useModelChoice(s);
  const [mode, setMode] = useSendMode();
  const queue = async () => {
    const res = await attempt('キューへの追加', () => api.queue(s.id, text));
    if (res) toast(res.queued ? '保留しました。Claude が入力待ちになったら送ります' : 'Claude が入力待ちなので、すぐに送りました');
    return !!res;
  };
  const switchAndSend = async () => {
    const res = await attempt('モデルと effort の切り替え', () => api.switchModel(s.id, text, choice.model, choice.effort));
    if (!res) return false;
    choice.reset();
    toast(res.sessionId === s.id ? 'モデルと effort を変えて届けました' : 'モデルと effort を変えた複製に乗り換えて届けました');
    // このセッションの画面を開いていれば、会話が続く複製の画面へ移る。
    if (res.sessionId !== s.id && currentSessionId() === s.id) location.hash = href.session(res.sessionId);
    return true;
  };
  const send = async () => {
    if (busy || !text.trim()) return;
    setBusy(true);
    const res = mode === 'queue' ? await queue() : choice.changed ? await switchAndSend() : await attempt('メッセージの送信', () => api.message(s.id, text));
    setBusy(false);
    if (!res) return;
    setText('');
    if (res !== true) toast(DELIVERY_TOAST[res.delivery]);
  };
  const fork = async () => {
    if (busy || !text.trim()) return;
    setBusy(true);
    const res = await attempt('セッションの分岐', () => api.fork(s.id, text, choice.model, choice.effort));
    setBusy(false);
    if (!res) return;
    setText('');
    choice.reset();
    toast('分岐したセッションに届けました');
    // このセッションの画面を開いていれば、分岐したセッションの画面へ移る。
    if (currentSessionId() === s.id) location.hash = href.session(res.sessionId);
  };
  // compact した会話は戻せないので、押し間違いでないかを確かめる。
  const compact = async () => {
    if (!confirm('このセッションの会話を compact しますか?')) return;
    setBusy(true);
    const done = await attempt('compact', () => api.compact(s.id).then(() => true));
    setBusy(false);
    if (done) toast('compact を始めました');
  };
  // 書きかけの文を消さないよう、置き換えるのは空か候補のままのときに限り、それ以外は後ろに足す。
  const pick = (reply: string, replies: string[]) => {
    setText((t) => (!t.trim() || replies.includes(t) ? reply : `${t}\n${reply}`));
    input.current?.focus();
  };
  return (
    <>
      <ReplySuggestions s={s} onPick={pick} />
      <MessageInput sessionId={s.id} value={text} setValue={setText} placeholder="メッセージ（/ で skills、# で PR と Issue を補完、Cmd/Ctrl + Enter で送信）" onSubmit={send} inputRef={input} />
      <div class="actions">
        <span class="send-split">
          <button class="btn btn-primary" disabled={busy || !text.trim()} title={mode === 'queue' ? QUEUE_HELP : choice.changed ? SWITCH_HELP : undefined} onClick={send}>
            {mode === 'queue' ? 'キュー' : choice.changed ? '切り替えて送信' : '送信'}
          </button>
          <SendModeMenu mode={mode} onChange={setMode} />
        </span>
        <button class="btn" disabled={busy || !text.trim()} title={FORK_HELP} onClick={fork}>分岐</button>
        {choice.fields}
        <ContextPie usage={s.usage} />
        <button class="btn" disabled={busy} title="会話を要約してコンテキストを空ける" onClick={compact}>compact</button>
      </div>
    </>
  );
};

/**
 * Claude が人間の操作を待って止まっている、そのセッションの質問と権限の確認。
 * 質問を読み取れずに権限の確認として届いたときも、止まっていることがチャットで分かるよう、権限の確認も含める。
 */
export const asksOf = (attention: Attention[], sessionId: string): Attention[] =>
  attention.filter((a) => a.sessionId === sessionId && (a.kind === 'ask' || a.kind === 'question' || a.kind === 'permission'));

/** 保留しているメッセージ。書き直している間はサーバーが送らないので、書き直しを始めるときと終えるときに知らせる。 */
const QueuedItem = ({ m }: { m: QueuedMessage }) => {
  const [draft, setDraft] = useState<string>();
  const [busy, setBusy] = useState(false);
  const run = async (label: string, fn: () => Promise<unknown>): Promise<boolean> => {
    setBusy(true);
    const done = await attempt(label, () => fn().then(() => true));
    setBusy(false);
    return !!done;
  };
  const edit = async () => {
    if (await run('書き直しの開始', () => api.editQueued(m.id))) setDraft(m.text);
  };
  // 取りやめるときも、元の文で保存して書き直しを終え、送れるようにする。
  const save = async (text: string) => {
    if (text.trim() && (await run('書き直した文の保存', () => api.saveQueued(m.id, text)))) setDraft(undefined);
  };
  if (draft !== undefined) {
    return (
      <li class="outbox-item is-queued">
        <MessageInput sessionId={m.sessionId} value={draft} setValue={setDraft} placeholder="書き直した文（Cmd/Ctrl + Enter で保存）" onSubmit={() => save(draft)} />
        <div class="actions">
          <button class="btn btn-sm" disabled={busy} onClick={() => save(m.text)}>キャンセル</button>
          <button class="btn btn-sm btn-primary" disabled={busy || !draft.trim()} onClick={() => save(draft)}>保存</button>
        </div>
      </li>
    );
  }
  return (
    <li class="outbox-item is-queued">
      <p class="plain">{m.text}</p>
      <div class="outbox-meta">
        <span class="muted">{m.editing ? '書き直し中のため送りません' : '保留中'} · <Time at={m.createdAt} /></span>
        <span class="spacer" />
        <button class="btn btn-sm" disabled={busy} onClick={edit}>編集</button>
        <button class="btn btn-sm" disabled={busy} onClick={() => run('取り消し', () => api.cancelQueued(m.id))}>取り消し</button>
      </div>
    </li>
  );
};

/** 「キュー」で保留しているメッセージ。Claude が入力待ちになったら、上から 1 つずつ送られる。 */
const QueuedList = ({ s }: { s: Session }) =>
  s.queued?.length ? (
    <ul class="outbox queued-list" aria-label="保留しているメッセージ">
      {s.queued.map((m) => <QueuedItem key={m.id} m={m} />)}
    </ul>
  ) : null;

/**
 * 送信欄。Claude が質問の答えを待っている間は、質問のカードの回答欄から答えてもらい、通常の送信欄は出さない。
 * 両方を出すと、どちらで答えればよいか迷う。権限の確認のカードには回答欄が無いので、送信欄も出す。
 * チャットのウィジェットと、ヘッダーの件数から開く一覧で使う。
 */
export const ReplyArea = ({ s, asks, help }: { s: Session; asks: Attention[]; help?: boolean }) => (
  <>
    <QueuedList s={s} />
    {asks.map((a) => <AttentionCard key={a.id} a={a} session={s} />)}
    {asks.every((a) => a.kind === 'permission') && (
      <>
        {help && <p class="help">{STATUS_HELP[s.status]}</p>}
        <MessageForm s={s} />
      </>
    )}
  </>
);

/** チャットのウィジェットの下端に置く送信欄。 */
export const Composer = ({ s, asks }: { s: Session; asks: Attention[] }) => (
  <div class="composer">
    <ReplyArea s={s} asks={asks} help />
  </div>
);

const PendingItem = ({ m, remoteUrl }: { m: OutboxMessage; remoteUrl?: string }) => {
  const [busy, setBusy] = useState(false);
  // コピーと新しいタブは、クリックの操作中に始めないとブラウザに止められることがある。
  const deliverByHand = async () => {
    setBusy(true);
    const copied = copyText(m.text);
    if (remoteUrl) window.open(remoteUrl, '_blank', 'noopener');
    if (await copied) {
      toast('コピーしました。claude.ai/code に貼り付けて送ってください');
      await attempt('手動配信の記録', () => api.manual(m.id));
    } else {
      toast('クリップボードにコピーできませんでした', 'error');
    }
    setBusy(false);
  };
  const retry = async () => {
    setBusy(true);
    await attempt('再送', () => api.retry(m.id));
    setBusy(false);
  };
  const cancel = async () => {
    setBusy(true);
    await attempt('取り消し', () => api.cancel(m.id));
    setBusy(false);
  };
  return (
    <li class="outbox-item is-pending">
      <p class="plain">{m.text}</p>
      {m.error && <p class="help error-text">届けられませんでした: {m.error}</p>}
      <div class="outbox-meta">
        <span class="muted">未配信 · <Time at={m.createdAt} /></span>
        <span class="spacer" />
        <button class="btn btn-sm btn-primary" disabled={busy} onClick={retry}>再送</button>
        <button class="btn btn-sm" disabled={busy} onClick={deliverByHand}>
          {remoteUrl ? 'コピーして claude.ai/code を開く' : 'コピーして手動で届ける'}
        </button>
        <button class="btn btn-sm" disabled={busy} onClick={cancel}>取り消し</button>
      </div>
    </li>
  );
};

/** 送ったメッセージが会話に現れた位置。送った後の依頼のうち、文面を含む最初のもの。 */
const chatIndexOf = (chat: ChatEntry[], m: OutboxMessage): number =>
  chat.findIndex((e) => e.kind === 'prompt' && (e.at ?? m.createdAt) >= m.createdAt && e.text.includes(m.text.trim()));

/** 会話に現れたメッセージは、どこを押してもチャットのその発言へ移れる。 */
const DeliveredItem = ({ m, onJump }: { m: OutboxMessage; onJump?: () => void }) => (
  <li class="outbox-item">
    <p class="plain">{m.text}</p>
    <div class="outbox-meta muted">
      配信済み{m.via && ` · ${VIA_LABEL[m.via]}`}
      {m.deliveredAt && ` · ${dateTime(m.deliveredAt)}`}
      {onJump && <button class="outbox-jump stretched" onClick={onJump}>チャットで見る</button>}
    </div>
  </li>
);

/** entries はチャットの発言。 */
type OutboxListProps = { s: Session; outbox: OutboxMessage[]; entries: ChatEntry[]; onJump: (index: number) => void };

export const OutboxList = ({ s, outbox, entries, onJump }: OutboxListProps) => {
  const mine = outbox.filter((m) => m.sessionId === s.id).sort((a, b) => b.createdAt - a.createdAt);
  const pending = mine.filter((m) => !m.deliveredAt);
  const delivered = mine.filter((m) => m.deliveredAt);
  return (
    <WidgetPanel title="送信したメッセージ" count={mine.length}>
      {mine.length === 0 && <Empty>このセッションへ送ったメッセージはありません。</Empty>}
      <ul class="outbox">
        {pending.map((m) => <PendingItem key={m.id} m={m} remoteUrl={s.remoteUrl} />)}
        {delivered.slice(0, DELIVERED_LIMIT).map((m) => {
          const index = chatIndexOf(entries, m);
          return <DeliveredItem key={m.id} m={m} onJump={index < 0 ? undefined : () => onJump(index)} />;
        })}
      </ul>
      {delivered.length > DELIVERED_LIMIT && <p class="help">古い {delivered.length - DELIVERED_LIMIT} 件は省略しています。</p>}
    </WidgetPanel>
  );
};
