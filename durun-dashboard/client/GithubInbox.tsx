// ダッシュボードの「GitHub Inbox」ウィジェット。GitHub の PR のダッシュボードの Inbox と同じ区分で、自分に関わる開いた PR を並べる。
import { useEffect } from 'preact/hooks';
import type { InboxPr } from '../shared/types.ts';
import { api } from './api.ts';
import type { Control } from './Float.tsx';
import { useCached } from './hooks.ts';
import { Empty, Ext, Time } from './ui.tsx';

export const GITHUB_INBOX_TITLE = 'GitHub Inbox';
// タイトルバーのボタンはウィジェットの外にあるので、イベントで探し直させる。
const REFRESH_EVENT = 'github-inbox-refresh';

export const githubInboxControls = (): Control[] => [
  { kind: 'refresh', label: 'GitHub の PR を探し直す', onClick: () => dispatchEvent(new Event(REFRESH_EVENT)) },
];

/** 行を押すと、PR へのリンクと同じくフローティングウィンドウで開く。 */
const PrRow = ({ pr }: { pr: InboxPr }) => (
  <li>
    <Ext class="jira-row" href={pr.url}>
      <span class="jira-head">
        <span class="jira-key">{pr.repo} #{pr.number}</span>
        <span class="jira-meta">{pr.author} · <Time at={pr.updatedAt} /></span>
      </span>
      <span class="jira-summary">{pr.title}</span>
    </Ext>
  </li>
);

export const GithubInbox = () => {
  const { data, error, reload } = useCached(api.githubInbox, []);
  useEffect(() => {
    addEventListener(REFRESH_EVENT, reload);
    return () => removeEventListener(REFRESH_EVENT, reload);
  }, [reload]);
  return (
    <>
      <div class="section-head">
        <h2 class="section-title">{GITHUB_INBOX_TITLE}</h2>
      </div>
      <div class="widget-body">
        {error ? (
          <Empty>PR を探せませんでした（{error}）</Empty>
        ) : !data ? (
          <Empty>探しています…</Empty>
        ) : data.length === 0 ? (
          <Empty>自分に関わる開いた PR はありません。</Empty>
        ) : (
          data.map((s) => (
            <section key={s.title} class="inbox-section">
              <h3 class="inbox-section-title">
                {s.title}
                <span class="count-pill">{s.prs.length}</span>
              </h3>
              <ul class="jira-list">{s.prs.map((pr) => <PrRow key={pr.url} pr={pr} />)}</ul>
            </section>
          ))
        )}
      </div>
    </>
  );
};
