import type { ComponentChildren } from 'preact';
import { useState } from 'preact/hooks';
import type { Attention, AttentionKind, Session, Snapshot } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { Markdown, MarkdownPreview } from './Markdown.tsx';
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

const Body = ({ text }: { text?: string }) => (text ? <Markdown text={text} /> : null);

const AskBody = ({ a }: { a: Attention }) => {
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const send = async (answer: string) => {
    if (busy || !answer.trim()) return;
    setBusy(true);
    const res = await attempt('回答の送信', () => api.answer(a.id, answer));
    setBusy(false);
    if (!res) return;
    setText('');
    toast('回答を送りました');
  };
  return (
    <>
      <Body text={a.body !== a.title ? a.body : undefined} />
      {a.options && a.options.length > 0 && (
        <Actions>
          {a.options.map((o) => (
            <button key={o} class="btn att-option" disabled={busy} onClick={() => send(o)}>{o}</button>
          ))}
        </Actions>
      )}
      <textarea
        class="input"
        rows={2}
        placeholder="自由記述で回答（Cmd/Ctrl + Enter で送信）"
        value={text}
        onInput={(e) => setText(e.currentTarget.value)}
        onKeyDown={(e) => onSubmitKey(e, () => send(text))}
      />
      <Actions>
        <button class="btn btn-primary" disabled={busy || !text.trim()} onClick={() => send(text)}>送信</button>
      </Actions>
    </>
  );
};

const PermissionBody = ({ a, session }: { a: Attention; session?: Session }) => (
  <>
    <Body text={a.body} />
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
          <Body text={a.body} />
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
