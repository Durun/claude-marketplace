import type { ComponentChildren } from 'preact';
import { useMemo, useState } from 'preact/hooks';
import type { AskedQuestion, Attention, AttentionKind, Session, Snapshot } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { fileLink, filePathMarks } from './Docs.tsx';
import { Markdown, MarkdownPreview } from './Markdown.tsx';
import { toggled } from './explorerFavorites.ts';
import { askAnswer } from './format.ts';
import { KIND_LABEL } from './notify.ts';
import { href } from './router.ts';
import { toast } from './toast.tsx';
import { Empty, Ext, Time, onSubmitKey } from './ui.tsx';

const KIND_ORDER: AttentionKind[] = ['ask', 'question', 'review', 'permission', 'pr', 'reply', 'doc'];
const REPLY_PREVIEW_CHARS = 400;

const sortAttention = (list: Attention[]): Attention[] =>
  [...list].sort((a, b) => KIND_ORDER.indexOf(a.kind) - KIND_ORDER.indexOf(b.kind) || b.createdAt - a.createdAt);

const Actions = ({ children }: { children: ComponentChildren }) => <div class="actions">{children}</div>;

const DismissButton = ({ id }: { id: string }) => {
  const [busy, setBusy] = useState(false);
  const click = async () => {
    setBusy(true);
    await attempt('既読にする操作', () => api.dismiss(id));
    setBusy(false);
  };
  return <button class="btn btn-sm" disabled={busy} onClick={click}>既読</button>;
};

/** 文に書いたファイルパスを、インラインのコードでも地の文でも開けるようにする。id はセッションの id。 */
const Body = ({ id, text }: { id: string; text?: string }) => {
  const links = useMemo(() => ({ codeLink: fileLink(id), markText: filePathMarks(id) }), [id]);
  return text ? <Markdown text={text} {...links} /> : null;
};

/** AskUserQuestion の選択肢は、Claude Code の画面と同じく、見出しの下に説明を添える。 */
const OptionLabel = ({ label, description }: { label: string; description?: string }) => (
  <>
    {label}
    {description && <small class="att-option-desc">{description}</small>}
  </>
);

type QuestionTabsProps = { id: string; questions: AskedQuestion[]; picked: string[][]; tab: number; busy: boolean; setTab: (tab: number) => void; pick: (option: string) => void };

/** 分けた質問を、Claude Code の AskUserQuestion のようにタブで 1 つずつ出す。最後のタブで回答を見直す。 */
const QuestionTabs = ({ id, questions, picked, tab, busy, setTab, pick }: QuestionTabsProps) => {
  const q = questions[tab];
  const labelOf = (x: AskedQuestion, i: number): string => x.header ?? `質問 ${i + 1}`;
  return (
    <>
      <div class="pick-tabs" role="tablist" aria-label="質問">
        {questions.map((x, i) => (
          <button key={i} role="tab" aria-selected={i === tab} onClick={() => setTab(i)}>{picked[i]?.length ? '✓ ' : ''}{labelOf(x, i)}</button>
        ))}
        <button role="tab" aria-selected={!q} onClick={() => setTab(questions.length)}>送信</button>
      </div>
      {q ? (
        <>
          <Body id={id} text={q.question} />
          <Actions>
            {q.options.map((o, i) => {
              const on = !!picked[tab]?.includes(o);
              return (
                <button key={o} class={`btn att-option${on ? ' btn-primary' : ''}`} aria-pressed={on} disabled={busy} onClick={() => pick(o)}>
                  <OptionLabel label={o} description={q.descriptions?.[i]} />
                </button>
              );
            })}
            {q.multiSelect && <button class="btn" onClick={() => setTab(tab + 1)}>次へ</button>}
          </Actions>
        </>
      ) : (
        <div class="md">
          <ul>{questions.map((x, i) => <li key={i}>{labelOf(x, i)}: {picked[i]?.join('、') || '未回答'}</li>)}</ul>
        </div>
      )}
    </>
  );
};

const AskBody = ({ a }: { a: Attention }) => {
  const [text, setText] = useState('');
  const [picked, setPicked] = useState<string[][]>([]);
  const [tab, setTab] = useState(0);
  const [busy, setBusy] = useState(false);
  const questions = a.questions ?? [];
  // 質問を分けたときは、全部に答えるか自由記述を書けば送れる。
  const ready = (questions.length > 0 && questions.every((_, i) => picked[i]?.length)) || !!text.trim();
  const send = async (answer: string) => {
    if (busy || !answer.trim()) return;
    setBusy(true);
    const res = await attempt('回答の送信', () => api.answer(a.id, answer));
    setBusy(false);
    if (!res) return;
    setText('');
    toast('回答を送りました');
  };
  const submit = () => ready && send(questions.length ? askAnswer(questions, picked, text) : text);
  // 1 つを選ぶ質問は、選ぶと次のタブへ進む。いくつでも選べる質問は、押すたびに選ぶか外す。
  const pick = (option: string) => {
    const multi = questions[tab]?.multiSelect;
    setPicked((p) => questions.map((_, i) => (i !== tab ? (p[i] ?? []) : multi ? toggled(p[i] ?? [], option) : [option])));
    if (!multi) setTab(tab + 1);
  };
  // AskUserQuestion の本文は質問と選択肢を並べたものなので、タブで出すときは重ねて出さない。
  const body = a.kind === 'question' && questions.length ? undefined : a.body;
  return (
    <>
      <Body id={a.sessionId} text={body !== a.title ? body : undefined} />
      {questions.length > 0 ? (
        <QuestionTabs id={a.sessionId} questions={questions} picked={picked} tab={tab} busy={busy} setTab={setTab} pick={pick} />
      ) : (
        a.options && a.options.length > 0 && (
          <Actions>
            {a.options.map((o, i) => (
              <button key={o} class="btn att-option" disabled={busy} onClick={() => send(o)}>
                <OptionLabel label={o} description={a.descriptions?.[i]} />
              </button>
            ))}
          </Actions>
        )
      )}
      {tab === questions.length && (
        <>
          <textarea
            class="input"
            rows={2}
            placeholder="自由記述で回答（Cmd/Ctrl + Enter で送信）"
            value={text}
            onInput={(e) => setText(e.currentTarget.value)}
            onKeyDown={(e) => onSubmitKey(e, submit)}
          />
          <Actions>
            <button class="btn btn-primary" disabled={busy || !ready} onClick={submit}>送信</button>
          </Actions>
        </>
      )}
    </>
  );
};

const PermissionBody = ({ a, session }: { a: Attention; session?: Session }) => (
  <>
    <Body id={a.sessionId} text={a.body} />
    <Actions>
      {session?.remoteUrl && <Ext class="btn btn-primary" href={session.remoteUrl}>claude.ai/code で開く</Ext>}
      <DismissButton id={a.id} />
    </Actions>
  </>
);

const ReplyBody = ({ a }: { a: Attention }) => (
  <>
    {a.body && <MarkdownPreview text={a.body} max={REPLY_PREVIEW_CHARS} />}
    <Actions>
      <a class="btn btn-primary" href={href.session(a.sessionId)}>返信する</a>
      <DismissButton id={a.id} />
    </Actions>
  </>
);

const AttentionBody = ({ a, session }: { a: Attention; session?: Session }) => {
  switch (a.kind) {
    case 'ask':
      return <AskBody a={a} />;
    case 'question':
      // バックグラウンドのセッションは、止めて再開すれば回答を渡せる。端末のセッションは止められないので、Claude Code の画面で答えてもらう。
      return session?.kind === 'bg' ? <AskBody a={a} /> : <PermissionBody a={a} session={session} />;
    case 'review':
      return <Actions><a class="btn btn-primary" href={href.session(a.sessionId, 'code')}>コードを見る</a></Actions>;
    case 'permission':
      return <PermissionBody a={a} session={session} />;
    case 'reply':
      return <ReplyBody a={a} />;
    case 'pr':
      return (
        <>
          <Body id={a.sessionId} text={a.body} />
          <Actions>
            {a.url && <Ext class="btn" href={a.url}>PR を開く</Ext>}
            <DismissButton id={a.id} />
          </Actions>
        </>
      );
    case 'doc':
      return <Actions><a class="btn btn-primary" href={a.docId ? href.doc(a.docId) : href.docs()}>開く</a></Actions>;
  }
};

export const AttentionCard = ({ a, session }: { a: Attention; session?: Session }) => (
  <article class={`card att att-${a.kind}`}>
    <header class="att-head">
      <span class="att-kind">{KIND_LABEL[a.kind]}</span>
      <a class="att-session" href={href.session(a.sessionId)}>{session?.name ?? a.sessionId}</a>
      <Time at={a.createdAt} />
    </header>
    <h3 class="att-title">{a.title}</h3>
    <AttentionBody a={a} session={session} />
  </article>
);

export const Inbox = ({ snapshot }: { snapshot: Snapshot }) => {
  if (snapshot.attention.length === 0) return <Empty>対応が必要なものはありません。</Empty>;
  const byId = new Map(snapshot.sessions.map((s) => [s.id, s]));
  return (
    <div class="stack">
      {sortAttention(snapshot.attention).map((a) => (
        <AttentionCard key={a.id} a={a} session={byId.get(a.sessionId)} />
      ))}
    </div>
  );
};
