// JIRA の課題を、本文とコメントとともに読み、コメントを投稿する。
// セッションの課題はセッション内の「JIRA」ウィジェットで、ほかの課題はウィンドウに浮かべて開く。
import { useEffect, useState } from 'preact/hooks';
import type { JiraDetail, Session } from '../shared/types.ts';
import { api } from './api.ts';
import { WidgetPanel } from './Dashboard.tsx';
import { type Float, FloatFrame, JIRA_ICON, dock } from './Float.tsx';
import { useFetch } from './hooks.ts';
import { IssueRow } from './JiraIssues.tsx';
import { Markdown } from './Markdown.tsx';
import { CommentForm } from './PrView.tsx';
import { Chip, Empty, Time } from './ui.tsx';

export const JIRA_VIEW_TITLE = 'JIRA';
/** セッション内の「JIRA」ウィジェットの種類の名前。 */
export const JIRA_WIDGET = 'jira';
/** ウィンドウに浮かべた課題の保存キー。タブの id に課題のキーを入れる。 */
export const JIRA_KEY = 'jira';

/** セッションのウィジェットで選ぶよう頼まれた課題。セッションの id ごとに持つ。 */
const picked = new Map<string, string>();
const pickListeners = new Set<() => void>();

/** セッションに課題が複数あるとき、ウィジェットでその課題を選ぶ。 */
export const pickJira = (session: string, key: string): void => {
  picked.set(session, key);
  pickListeners.forEach((fn) => fn());
};

const CATEGORY_TONE: Record<string, 'muted' | 'info' | 'ok' | undefined> = { new: 'muted', indeterminate: 'info', done: 'ok' };

const CHILD_FIELDS = new Set(['assignee', 'priority']);

/** JIRA で開くリンクは、課題へのリンクを横取りする処理に拾わせないよう data-external を付ける。 */
const Detail = ({ issue, loading, reload }: { issue: JiraDetail; loading: boolean; reload: () => void }) => (
  <div class="stack pr-view">
    <div class="pr-view-head">
      <a class="pr-view-title" href={issue.url} target="_blank" rel="noopener noreferrer" data-external>{issue.key} {issue.summary}</a>
      <span class="spacer" />
      <button class="btn btn-sm" data-icon="refresh" disabled={loading} title="JIRA から読み直します" onClick={reload}>更新</button>
    </div>
    <div class="pr-view-meta muted">
      {issue.status && <Chip tone={CATEGORY_TONE[issue.statusCategory ?? '']}>{issue.status}</Chip>}
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
  const issue = useFetch(() => api.jiraIssue(issueKey), [issueKey, version]);
  if (issue.data) return <Detail issue={issue.data} loading={issue.loading} reload={issue.reload} />;
  return <Empty>{issue.error ? `課題を読めませんでした（${issue.error}）` : '読み込んでいます…'}</Empty>;
};

/** セッションに課題が複数あるときは、上に並べたキーで切り替える。状態が変わったら読み直す。 */
export const JiraWidget = ({ s }: { s: Session }) => {
  const [pick, setPick] = useState(picked.get(s.id));
  useEffect(() => {
    const take = () => setPick(picked.get(s.id));
    take();
    pickListeners.add(take);
    return () => {
      pickListeners.delete(take);
    };
  }, [s.id]);
  const issue = s.jira.find((j) => j.key === pick) ?? s.jira[0];
  return (
    <WidgetPanel title={JIRA_VIEW_TITLE}>
      {s.jira.length > 1 && (
        <div class="chips">
          {s.jira.map((j) => (
            <button key={j.key} class="btn btn-sm" aria-pressed={j === issue} title={j.summary} onClick={() => pickJira(s.id, j.key)}>{j.key}</button>
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
