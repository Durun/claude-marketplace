// ダッシュボードの「GitHub Inbox」ウィジェット。設定した検索の条件で PR と Issue を探して並べる。共通の形は ListWidget にある。
import { useEffect } from 'preact/hooks';
import { GITHUB_LIST_MAX, type GithubItem } from '../shared/types.ts';
import { api } from './api.ts';
import type { Control } from './Float.tsx';
import type { HomeWidgetProps } from './Home.tsx';
import { useCached } from './hooks.ts';
import { CHECKS_LABEL, GITHUB_GROUPINGS, type Grouping, type ItemGroup, REVIEW_LABEL } from './listGroups.ts';
import { type ListSpec, ListWidget, keysOf } from './ListWidget.tsx';
import { Chip, Ext, Time } from './ui.tsx';

export const GITHUB_INBOX_TITLE = 'GitHub Inbox';
// タイトルバーのボタンはウィジェットの外にあるので、イベントで探し直させる。
const REFRESH_EVENT = 'github-inbox-refresh';
// 自分に関わる開いた PR を、更新が新しい順に探す。
const DEFAULT_QUERY = 'is:pr is:open archived:false involves:@me sort:updated-desc';
const DEFAULT_MAX = 50;

export const githubInboxControls = (): Control[] => [
  { kind: 'refresh', label: 'GitHub の PR と Issue を探し直す', onClick: () => dispatchEvent(new Event(REFRESH_EVENT)) },
];

/** 一覧の行に出せる属性。設定では、選んだものの key をカンマで区切って shown に持つ。 */
const FIELDS = [
  { key: 'kind', label: 'PR/Issue' },
  { key: 'draft', label: 'ドラフト' },
  { key: 'review', label: 'レビューの判定' },
  { key: 'checks', label: 'チェックの状態' },
  { key: 'author', label: '作成者' },
] as const;

const REVIEW_TONE: Record<string, 'ok' | 'bad' | 'warn' | undefined> = { APPROVED: 'ok', CHANGES_REQUESTED: 'bad', REVIEW_REQUIRED: 'warn' };
const CHECKS_TONE: Record<string, 'ok' | 'bad' | 'info' | undefined> = { SUCCESS: 'ok', FAILURE: 'bad', ERROR: 'bad', PENDING: 'info', EXPECTED: 'info' };

/** 行を押すと、PR へのリンクと同じくフローティングウィンドウで開く。 */
const ItemRow = ({ item, fields }: { item: GithubItem; fields: Set<string> }) => (
  <li>
    <Ext class="jira-row" href={item.url}>
      <span class="jira-head">
        <span class="jira-key"><span class="jira-key-trim">{item.repo}</span> #{item.number}</span>
        {fields.has('kind') && <Chip>{item.kind === 'pr' ? 'PR' : 'Issue'}</Chip>}
        {fields.has('draft') && item.isDraft && <Chip tone="muted">ドラフト</Chip>}
        {fields.has('review') && item.review && <Chip tone={REVIEW_TONE[item.review]}>{REVIEW_LABEL[item.review] ?? item.review}</Chip>}
        {fields.has('checks') && item.checks && <Chip tone={CHECKS_TONE[item.checks]}>{CHECKS_LABEL[item.checks] ?? item.checks}</Chip>}
        <span class="jira-rule" aria-hidden="true" />
        <span class="jira-meta">
          {fields.has('author') && <>{item.author} · </>}
          <Time at={item.updatedAt} />
        </span>
      </span>
      <span class="jira-summary">{item.title}</span>
    </Ext>
  </li>
);

/** 組の見出しの名前。レビューの判定とチェックの状態は、行と同じ色で出す。 */
const GroupName = ({ by, group }: { by: Grouping<GithubItem>; group: ItemGroup<GithubItem> }) => {
  // 組の項目はどれも同じ値を持つので、先頭の項目から色を決める。値の無い組は名前だけを出す。
  const [item] = group.items;
  if (by.key === 'review' && item?.review) return <Chip tone={REVIEW_TONE[item.review]}>{group.name}</Chip>;
  if (by.key === 'checks' && item?.checks) return <Chip tone={CHECKS_TONE[item.checks]}>{group.name}</Chip>;
  return <>{group.name}</>;
};

const GITHUB_SPEC: ListSpec<GithubItem> = {
  title: GITHUB_INBOX_TITLE,
  noun: 'PR と Issue',
  query: { key: 'query', label: '条件（GitHub の検索）', fallback: DEFAULT_QUERY },
  maxLimit: GITHUB_LIST_MAX,
  defaultMax: DEFAULT_MAX,
  fields: FIELDS,
  // 設定が無いときは、属性を選べるようになる前と同じく作成者だけを出す。
  fieldsOf: (settings) => keysOf(settings.shown, 'author'),
  groupings: GITHUB_GROUPINGS,
  groupKeysOf: (settings) => keysOf(settings.groups),
  keyOf: (i) => i.url,
  textOf: (i) => [i.repo, `#${i.number}`, i.title, i.author, i.section],
  Row: ({ item, fields }) => <ItemRow item={item} fields={fields} />,
  GroupName,
};

export const GithubInbox = ({ settings, onSettings }: HomeWidgetProps) => (
  <ListWidget
    spec={GITHUB_SPEC}
    settings={settings}
    onSettings={onSettings}
    useFound={(query, max) => {
      const { data, error, reload } = useCached((fresh) => api.githubSearch(query, max, fresh), [query, max]);
      useEffect(() => {
        addEventListener(REFRESH_EVENT, reload);
        return () => removeEventListener(REFRESH_EVENT, reload);
      }, [reload]);
      return { items: data?.items, more: (data?.total ?? 0) > (data?.items.length ?? 0), error };
    }}
  />
);
