// GitHub の PR を、本文・変更したファイル・会話とともに読み、会話にコメントを投稿する。
// セッションの PR はセッション内の「PR」ウィジェットで、ほかの PR はウィンドウに浮かべて開く。
import { useState } from 'preact/hooks';
import type { PrDetail, Session } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { WidgetPanel } from './Dashboard.tsx';
import { type Float, FloatFrame, GITHUB_ICON, dock } from './Float.tsx';
import { useFetch } from './hooks.ts';
import { Markdown } from './Markdown.tsx';
import { toast } from './toast.tsx';
import { Chip, Empty, Loading, Time, onSubmitKey, PrChips } from './ui.tsx';

export const PR_TITLE = 'PR';
/** セッション内の「PR」ウィジェットの種類の名前。 */
export const PR_WIDGET = 'pull';
/** ウィンドウに浮かべた PR の保存キー。タブの id に PR の URL を入れる。 */
export const PR_KEY = 'pr';

const REVIEW_STATE: Record<string, { text: string; tone: 'ok' | 'bad' | 'muted' } | undefined> = {
  APPROVED: { text: '承認', tone: 'ok' },
  CHANGES_REQUESTED: { text: '変更要求', tone: 'bad' },
  DISMISSED: { text: '取り下げ', tone: 'muted' },
};

/** target は「PR」や「課題」など、コメントする先の呼び名。JIRA の課題の画面でも使う。 */
export const CommentForm = ({ target, send, onPosted }: { target: string; send: (text: string) => Promise<unknown>; onPosted: () => void }) => {
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const post = async () => {
    if (busy || !text.trim()) return;
    setBusy(true);
    const res = await attempt(`${target}へのコメント`, async () => (await send(text), true));
    setBusy(false);
    if (!res) return;
    setText('');
    toast(`${target}にコメントしました`);
    onPosted();
  };
  return (
    <div class="stack pr-view-form">
      <textarea
        class="input"
        rows={3}
        placeholder={`${target}にコメントします（Cmd/Ctrl + Enter で投稿）`}
        value={text}
        onInput={(e) => setText(e.currentTarget.value)}
        onKeyDown={(e) => onSubmitKey(e, post)}
      />
      <div class="actions">
        <button class="btn btn-sm btn-primary" disabled={busy || !text.trim()} onClick={post}>コメントする</button>
      </div>
    </div>
  );
};

/** GitHub で開くリンクは、PR へのリンクを横取りする処理に拾わせないよう data-external を付ける。 */
const Detail = ({ pr, loading, reload }: { pr: PrDetail; loading: boolean; reload: () => void }) => (
  <div class="stack pr-view">
    <div class="pr-view-head">
      <a class="pr-view-title" href={pr.url} target="_blank" rel="noopener noreferrer" data-external>{pr.title}</a>
      <span class="spacer" />
      <button class="btn btn-sm" data-icon="refresh" disabled={loading} title="GitHub から読み直します" onClick={reload}>更新</button>
    </div>
    <div class="pr-view-meta muted">
      <PrChips pr={pr} />
      <span class="mono">{pr.headRef} → {pr.baseRef}</span>
      <span>{pr.author} · 更新 <Time at={pr.updatedAt} /></span>
      <span class="mono"><span class="stat-add">+{pr.additions}</span> <span class="stat-del">−{pr.deletions}</span></span>
    </div>
    {pr.body.trim() ? <Markdown text={pr.body} /> : <Empty>本文はありません。</Empty>}
    <details class="pr-view-files">
      <summary>変更したファイル（{pr.files.length}）</summary>
      <ul>
        {pr.files.map((f) => (
          <li key={f.path}>
            <span class="mono">{f.path}</span>
            <span class="mono"><span class="stat-add">+{f.additions}</span> <span class="stat-del">−{f.deletions}</span></span>
          </li>
        ))}
      </ul>
    </details>
    <ul class="pr-view-posts">
      {pr.posts.map((p) => (
        <li key={`${p.author}${p.at}`} class="pr-comment">
          <span class="muted">
            {p.author} · <Time at={p.at} />
            {p.state && REVIEW_STATE[p.state] && <> <Chip tone={REVIEW_STATE[p.state]?.tone}>{REVIEW_STATE[p.state]?.text}</Chip></>}
          </span>
          {p.body.trim() && <Markdown text={p.body} />}
        </li>
      ))}
    </ul>
    <CommentForm target="PR " send={(text) => api.prComment(pr.url, text)} onPosted={reload} />
  </div>
);

/** 開いたときと「更新」を押したとき、version が変わったときに GitHub から読み直す。 */
const PrView = ({ url, version }: { url: string; version?: string }) => {
  const pr = useFetch(() => api.prDetail(url), [url, version]);
  if (pr.data) return <Detail pr={pr.data} loading={pr.loading} reload={pr.reload} />;
  return pr.error ? <Empty>PR を読めませんでした（{pr.error}）</Empty> : <Loading />;
};

/** セッションの PR の状態（CI、レビュー）がスナップショットで変わったら、読み直す。 */
export const PrWidget = ({ s }: { s: Session }) => (
  <WidgetPanel title={PR_TITLE}>
    {s.pr ? <PrView key={s.pr.url} url={s.pr.url} version={JSON.stringify(s.pr)} /> : <Empty>このセッションのブランチには PR がありません。</Empty>}
  </WidgetPanel>
);

/** URL の owner/repo#番号 を見出しにする。 */
export const PrFloat = ({ f }: { f: Float }) => {
  const [, repo, num] = /^https:\/\/[^/]+\/([^/]+\/[^/]+)\/pull\/(\d+)/.exec(f.tab.id) ?? [];
  const close = { kind: 'close' as const, label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) };
  return (
    <FloatFrame f={f} title={`${repo}#${num}`} icon={GITHUB_ICON} controls={[close]}>
      <div class="widget-body">
        <PrView url={f.tab.id} />
      </div>
    </FloatFrame>
  );
};
