// セッションの PR に付いた、未解決のレビューコメントのウィジェット。
// スレッドごとに指摘の説明、事実確認、対応案を添え、人間が対応案を承認するか、指示を書いて、Claude に対応させる。
import { useState } from 'preact/hooks';
import type { OutboxMessage, PrInfo, PrThread, PrThreadNote, Session } from '../shared/types.ts';
import { DELIVERY_TOAST, api, attempt } from './api.ts';
import { headLines } from './format.ts';
import { useCached, useFetch } from './hooks.ts';
import { Markdown, MarkdownPreview } from './Markdown.tsx';
import { PrPicker, usePickedPr } from './PrView.tsx';
import { href } from './router.ts';
import { toast } from './toast.tsx';
import { Chip, Empty, Ext, Time, onSubmitKey } from './ui.tsx';

export const PR_REVIEW_TITLE = 'PR コメント';
const PREVIEW_CHARS = 300;
// 依頼に引く指摘の長さ。全文は URL から読める。
const QUOTE_CHARS = 1500;

const where = (t: PrThread): string => (t.line ? `${t.path}:${t.line}` : t.path);

/**
 * Claude に送る依頼。指示が無ければ対応案のとおりに進めさせ、あれば指示を優先させる。
 * 送信したメッセージから、どのスレッドへの依頼かを見分けられるよう、指摘の URL を入れる。
 */
const threadRequest = (pr: PrInfo, t: PrThread, note: PrThreadNote | undefined, instruction: string): string => {
  const [first] = t.comments;
  const order = instruction.trim();
  return [
    `PR #${pr.number} のレビューコメントに対応してください。`,
    `${first?.url ?? pr.url}（${where(t)}、${first?.author ?? ''} の指摘）`,
    headLines(first?.body ?? '', QUOTE_CHARS).replace(/^/gm, '> '),
    note && `ダッシュボードがコードを読んで作った対応案:\n${note.proposal}`,
    order ? `人間の指示${note ? '（対応案と食い違うときは、こちらを優先してください）' : ''}:\n${order}` : '対応案のとおりに進めてください。',
  ]
    .filter(Boolean)
    .join('\n\n');
};

/** このスレッドへ最後に送った依頼の時刻。 */
const sentAt = (outbox: OutboxMessage[], sessionId: string, t: PrThread): number | undefined => {
  const url = t.comments[0]?.url;
  return url ? outbox.findLast((m) => m.sessionId === sessionId && m.text.includes(url))?.createdAt : undefined;
};

type ThreadProps = { s: Session; pr: PrInfo; t: PrThread; note?: PrThreadNote; failed?: string; sent?: number };

const ThreadCard = ({ s, pr, t, note, failed, sent }: ThreadProps) => {
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const instructed = text.trim() !== '';
  const send = async () => {
    if (busy || (!note && !instructed)) return;
    setBusy(true);
    const res = await attempt('依頼の送信', () => api.message(s.id, threadRequest(pr, t, note, text)));
    setBusy(false);
    if (!res) return;
    setText('');
    toast(DELIVERY_TOAST[res.delivery]);
  };
  return (
    <li class="card pr-thread">
      <div class="pr-thread-head">
        <a class="mono" href={href.file(s.id, where(t))}>{where(t)}</a>
        {t.outdated && <Chip title="指摘の後にコードが変わり、指していた行は今の差分にありません">古い行</Chip>}
        {sent !== undefined && <Chip tone="info">依頼済み <Time at={sent} /></Chip>}
        <span class="spacer" />
        <Ext href={t.comments[0]?.url ?? pr.url}>GitHub で開く</Ext>
      </div>
      {t.comments.map((c) => (
        <div key={c.id} class="pr-comment">
          <span class="muted">{c.author} · <Time at={c.at} /></span>
          <MarkdownPreview text={c.body} max={PREVIEW_CHARS} />
        </div>
      ))}
      {note ? (
        <dl class="summary">
          <dt>指摘の説明</dt>
          <dd><Markdown text={note.explanation} /></dd>
          <dt>事実確認</dt>
          <dd><Markdown text={note.factCheck} /></dd>
          <dt>対応案</dt>
          <dd><Markdown text={note.proposal} /></dd>
        </dl>
      ) : (
        <p class="help">{failed ? `分析できませんでした（${failed}）` : 'コードを読んで、指摘を確かめています…'}</p>
      )}
      <textarea
        class="input"
        rows={2}
        placeholder="対応案と違う進め方をさせるときは、指示を書きます（Cmd/Ctrl + Enter で送信）"
        value={text}
        onInput={(e) => setText(e.currentTarget.value)}
        onKeyDown={(e) => onSubmitKey(e, send)}
      />
      <div class="actions">
        <button class="btn btn-sm btn-primary" disabled={busy || (!note && !instructed)} onClick={send}>
          {instructed ? '指示して対応させる' : '対応案で進める'}
        </button>
      </div>
    </li>
  );
};

type Props = { s: Session; outbox: OutboxMessage[] };

/**
 * 選んだ PR が開いていれば、未解決のスレッドを並べる。Claude が応答を始めたときと終えたとき、レビューの状態が変わったとき、「更新」を押したときに読み直す。
 * 分析は数分かかるので、スレッドを先に出す。スレッドに返信が付くと、分析を頼み直す。
 */
const PrThreads = ({ s, outbox, picked }: Props & { picked?: PrInfo }) => {
  const pr = picked?.state === 'OPEN' ? picked : undefined;
  const threads = useCached((fresh) => (pr ? api.prThreads(s.id, pr.url, fresh) : Promise.resolve([])), [pr?.url, pr?.review, s.status]);
  const list = threads.data ?? [];
  const key = list.map((t) => `${t.id}:${t.comments.at(-1)?.id}`).join();
  const notes = useFetch(() => (pr && list.length ? api.prNotes(s.id, pr.url) : Promise.resolve(undefined)), [key]);
  const failed = notes.loading ? undefined : (notes.data?.failed ?? notes.error);
  const reload = () => {
    threads.reload();
    notes.reload();
  };
  return (
    <>
      <div class="section-head">
        <h2 class="section-title">
          {PR_REVIEW_TITLE}
          {pr && threads.data && <span class="count-pill">{list.length}</span>}
        </h2>
        {pr && <button class="btn btn-sm" data-icon="refresh" disabled={threads.loading} title="PR のレビューコメントを読み直します" onClick={reload}>更新</button>}
      </div>
      <div class="widget-body">
        <PrPicker s={s} pr={picked} />
        {!picked ? (
          <Empty>このセッションのブランチには PR がありません。</Empty>
        ) : !pr ? (
          <Empty>PR #{picked.number} は{picked.state === 'MERGED' ? 'マージ済みです' : '閉じています'}。</Empty>
        ) : threads.error ? (
          <Empty>レビューコメントを読めませんでした（{threads.error}）</Empty>
        ) : !threads.data ? (
          <Empty>読み込んでいます…</Empty>
        ) : list.length === 0 ? (
          <Empty>未解決のレビューコメントはありません。</Empty>
        ) : (
          <ul class="pr-threads">
            {list.map((t) => (
              <ThreadCard key={t.id} s={s} pr={pr} t={t} note={notes.data?.notes[t.id]} failed={failed} sent={sentAt(outbox, s.id, t)} />
            ))}
          </ul>
        )}
      </div>
    </>
  );
};

/** セッションや PR を移ったら作り直し、前の PR のスレッドを出したまま依頼を送らせない。 */
export const PrReview = (props: Props) => {
  const picked = usePickedPr(props.s);
  return <PrThreads key={`${props.s.id} ${picked?.url}`} {...props} picked={picked} />;
};
