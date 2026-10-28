// JIRA の課題を、本文とコメントとともに読み、コメントを投稿する。
// セッションの課題はセッション内の「JIRA」ウィジェットで、ほかの課題はウィンドウに浮かべて開く。
import { useEffect, useId, useState } from 'preact/hooks';
import type { JiraDetail, JiraTransition, Session, Snapshot } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { WidgetPanel } from './Dashboard.tsx';
import { type Float, FloatFrame, JIRA_ICON, dock } from './Float.tsx';
import { pickInWidget, useCached, usePicked } from './hooks.ts';
import { IssueRow } from './JiraIssues.tsx';
import { Markdown } from './Markdown.tsx';
import { CommentForm, RelatedSessions } from './PrView.tsx';
import { toast } from './toast.tsx';
import { Chip, Empty, Loading, Time } from './ui.tsx';

export const JIRA_VIEW_TITLE = 'JIRA';
/** セッション内の「JIRA」ウィジェットの種類の名前。 */
export const JIRA_WIDGET = 'jira';
/** ウィンドウに浮かべた課題の保存キー。タブの id に課題のキーを入れる。 */
export const JIRA_KEY = 'jira';

const CATEGORY_TONE: Record<string, 'muted' | 'info' | 'ok' | undefined> = { new: 'muted', indeterminate: 'info', done: 'ok' };

const CHILD_FIELDS = new Set(['assignee', 'priority']);

/**
 * ステータスを押すと、課題を読んだときに JIRA が返した移せる先を並べる。選ぶと、そのトランジションで移してから課題を読み直す。
 * JIRA は移し終えるまで数秒かかることがあるので、選んだ先を待たずに出す。移せなかったら元のステータスに戻す。
 */
const StatusMenu = ({ issue, onMoved }: { issue: JiraDetail; onMoved: () => void }) => {
  const id = useId();
  // done は JIRA が移し終えたこと。読み直した課題が届くまでは課題のステータスも移せる先も古いので、選んだ先を出したまま選ばせない。
  const [moving, setMoving] = useState<{ to: JiraTransition; done?: boolean }>();
  useEffect(() => setMoving((m) => (m?.done ? undefined : m)), [issue]);
  const move = async (t: JiraTransition) => {
    setMoving({ to: t });
    const done = await attempt('ステータスの変更', () => api.jiraTransition(issue.key, t.id).then(() => true));
    setMoving(done ? { to: t, done } : undefined);
    if (!done) return;
    toast(`${issue.key} を「${t.status}」にしました`);
    onMoved();
  };
  const shown = moving?.to ?? issue;
  return (
    <span class="jira-status">
      <button class="jira-status-button" popovertarget={id} disabled={!!moving} title="押すと、ステータスを変えられます">
        <Chip tone={CATEGORY_TONE[shown.statusCategory ?? '']}>{shown.status}</Chip>
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

/** 開いたときと「更新」を押したとき、version が変わったときに JIRA から読み直す。related は、この課題をバッジにしているセッション。 */
const JiraView = ({ issueKey, version, related }: { issueKey: string; version?: string; related: Session[] }) => {
  const issue = useCached((fresh) => api.jiraIssue(issueKey, fresh), [issueKey, version]);
  return (
    <>
      <RelatedSessions sessions={related} />
      {issue.data ? <Detail issue={issue.data} loading={issue.loading} reload={issue.reload} /> : issue.error ? <Empty>課題を読めませんでした（{issue.error}）</Empty> : <Loading />}
    </>
  );
};

/** key の課題をバッジにしているセッション。here はウィジェットを出しているセッションで、今開いている画面なので並べない。 */
const jiraSessions = (sessions: Session[], key: string, here?: string): Session[] =>
  sessions.filter((s) => s.id !== here && s.jira.some((j) => j.key === key));

/** セッションに課題が複数あるときは、上に並べたキーのタブで切り替える。キーの左には、ステータスのチップと同じ色の丸を出す。状態が変わったら読み直す。 */
export const JiraWidget = ({ s, sessions }: { s: Session; sessions: Session[] }) => {
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
      {issue ? <JiraView key={issue.key} issueKey={issue.key} version={issue.status} related={jiraSessions(sessions, issue.key, s.id)} /> : <Empty>このセッションに関係する JIRA の課題はありません。</Empty>}
    </WidgetPanel>
  );
};

export const JiraFloat = ({ f, snapshot }: { f: Float; snapshot: Snapshot }) => {
  const close = { kind: 'close' as const, label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) };
  return (
    <FloatFrame f={f} title={f.tab.id} icon={JIRA_ICON} controls={[close]}>
      <div class="widget-body">
        <JiraView issueKey={f.tab.id} related={jiraSessions(snapshot.sessions, f.tab.id)} />
      </div>
    </FloatFrame>
  );
};
