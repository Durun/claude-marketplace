// JIRA の課題を、本文とコメントとともに読み、コメントを投稿する。
// セッションの課題はセッション内の「JIRA」ウィジェットで、ほかの課題はウィンドウに浮かべて開く。
import { useId, useState } from 'preact/hooks';
import type { JiraDetail, JiraTransition, Session } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { WidgetPanel } from './Dashboard.tsx';
import { type Float, FloatFrame, JIRA_ICON, dock } from './Float.tsx';
import { pickInWidget, useCached, usePicked } from './hooks.ts';
import { IssueRow } from './JiraIssues.tsx';
import { Markdown } from './Markdown.tsx';
import { CommentForm } from './PrView.tsx';
import { toast } from './toast.tsx';
import { Chip, Empty, Loading, Time } from './ui.tsx';

export const JIRA_VIEW_TITLE = 'JIRA';
/** セッション内の「JIRA」ウィジェットの種類の名前。 */
export const JIRA_WIDGET = 'jira';
/** ウィンドウに浮かべた課題の保存キー。タブの id に課題のキーを入れる。 */
export const JIRA_KEY = 'jira';

const CATEGORY_TONE: Record<string, 'muted' | 'info' | 'ok' | undefined> = { new: 'muted', indeterminate: 'info', done: 'ok' };

const CHILD_FIELDS = new Set(['assignee', 'priority']);

/** ステータスを押すと、課題を読んだときに JIRA が返した移せる先を並べる。選ぶと、そのトランジションで移してから課題を読み直す。 */
const StatusMenu = ({ issue, onMoved }: { issue: JiraDetail; onMoved: () => void }) => {
  const id = useId();
  const [busy, setBusy] = useState(false);
  const move = async (t: JiraTransition) => {
    setBusy(true);
    const done = await attempt('ステータスの変更', () => api.jiraTransition(issue.key, t.id).then(() => true));
    setBusy(false);
    if (!done) return;
    toast(`${issue.key} を「${t.status}」にしました`);
    onMoved();
  };
  return (
    <span class="jira-status">
      <button class="jira-status-button" popovertarget={id} disabled={busy} title="押すと、ステータスを変えられます">
        <Chip tone={CATEGORY_TONE[issue.statusCategory ?? '']}>{issue.status}</Chip>
      </button>
      <div id={id} class="jira-status-menu" popover>
        {issue.transitions.length === 0 && <p class="jira-status-note">移せるステータスはありません。</p>}
        {issue.transitions.map((t) => (
          <button key={t.id} class="jira-status-item" popovertarget={id} popovertargetaction="hide" onClick={() => void move(t)}>
            <Chip tone={CATEGORY_TONE[t.statusCategory ?? '']}>{t.status}</Chip>
            {t.name !== t.status && <span class="muted">{t.name}</span>}
          </button>
        ))}
      </div>
    </span>
  );
};

/** JIRA で開くリンクは、課題へのリンクを横取りする処理に拾わせないよう data-external を付ける。 */
const Detail = ({ issue, loading, reload }: { issue: JiraDetail; loading: boolean; reload: () => void }) => (
  <div class="stack pr-view jira-view">
    <div class="pr-view-head">
      <a class="pr-view-title" href={issue.url} target="_blank" rel="noopener noreferrer" data-external>{issue.key} {issue.summary}</a>
      <span class="spacer" />
      <button class="btn btn-sm" data-icon="refresh" disabled={loading} title="JIRA から読み直します" onClick={reload}>更新</button>
    </div>
    <div class="pr-view-meta muted">
      {issue.status && <StatusMenu issue={issue} onMoved={reload} />}
      {issue.labels?.map((l) => <Chip key={l}>{l}</Chip>)}
      <span>担当 {issue.assignee ?? '未割り当て'}</span>
      {issue.reporter && <span>報告 {issue.reporter}</span>}
      {issue.priority && <span>優先度 {issue.priority}</span>}
      {issue.parent && <span title={issue.parent.summary}>親 <a href={issue.url.replace(/[^/]+$/, issue.parent.key)}>{issue.parent.key}</a></span>}
      {issue.dueDate && <span>期限 {issue.dueDate}</span>}
      {issue.updatedAt !== undefined && <span>更新 <Time at={issue.updatedAt} /></span>}
    </div>
    {issue.description ? <Markdown text={issue.description} /> : <Empty>説明はありません。</Empty>}
    {issue.children.length > 0 && (
      <ul class="jira-list">{issue.children.map((c) => <IssueRow key={c.key} issue={c} fields={CHILD_FIELDS} />)}</ul>
    )}
    <ul class="pr-view-posts">
      {issue.comments.map((c) => (
        <li key={`${c.author}${c.at}`} class="pr-comment">
          <span class="muted">{c.author} · <Time at={c.at} /></span>
          <Markdown text={c.body} />
        </li>
      ))}
    </ul>
    <CommentForm target="課題" send={(text) => api.jiraComment(issue.key, text)} onPosted={reload} />
  </div>
);

/** 開いたときと「更新」を押したとき、version が変わったときに JIRA から読み直す。 */
const JiraView = ({ issueKey, version }: { issueKey: string; version?: string }) => {
  const issue = useCached((fresh) => api.jiraIssue(issueKey, fresh), [issueKey, version]);
  if (issue.data) return <Detail issue={issue.data} loading={issue.loading} reload={issue.reload} />;
  return issue.error ? <Empty>課題を読めませんでした（{issue.error}）</Empty> : <Loading />;
};

/** セッションに課題が複数あるときは、上に並べたキーのタブで切り替える。キーの左には、ステータスのチップと同じ色の丸を出す。状態が変わったら読み直す。 */
export const JiraWidget = ({ s }: { s: Session }) => {
  const pick = usePicked(JIRA_WIDGET, s.id);
  const issue = s.jira.find((j) => j.key === pick) ?? s.jira[0];
  return (
    <WidgetPanel title={JIRA_VIEW_TITLE}>
      {s.jira.length > 1 && (
        <div class="pick-tabs" role="tablist" aria-label="課題">
          {s.jira.map((j) => (
            <button key={j.key} role="tab" aria-selected={j === issue} title={j.summary} onClick={() => pickInWidget(JIRA_WIDGET, s.id, j.key)}>
              {j.status && <i class={`dot chip-${CATEGORY_TONE[j.statusCategory ?? ''] ?? 'muted'}`} role="img" aria-label={j.status} />}
              {j.key}
            </button>
          ))}
        </div>
      )}
      {issue ? <JiraView key={issue.key} issueKey={issue.key} version={issue.status} /> : <Empty>このセッションに関係する JIRA の課題はありません。</Empty>}
    </WidgetPanel>
  );
};

export const JiraFloat = ({ f }: { f: Float }) => {
  const close = { kind: 'close' as const, label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) };
  return (
    <FloatFrame f={f} title={f.tab.id} icon={JIRA_ICON} controls={[close]}>
      <div class="widget-body">
        <JiraView issueKey={f.tab.id} />
      </div>
    </FloatFrame>
  );
};
