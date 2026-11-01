// ダッシュボードの「JIRA Issues」ウィジェット。設定した JQL で JIRA の課題を探して並べる。共通の形は ListWidget にある。
import { JIRA_LIST_MAX, type JiraIssue } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { type Control, floatUp } from './Float.tsx';
import type { HomeWidgetProps } from './Home.tsx';
import { useCached } from './hooks.ts';
import { JIRA_KEY } from './JiraView.tsx';
import type { WidgetSettings } from './layout.ts';
import { type Grouping, type ItemGroup, JIRA_GROUPINGS } from './listGroups.ts';
import { type ListSpec, ListWidget, keysOf } from './ListWidget.tsx';
import { Chip, Ext, Time } from './ui.tsx';

export const JIRA_TITLE = 'JIRA Issues';
const DEFAULT_JQL = 'assignee = currentUser() AND statusCategory != Done ORDER BY updated DESC';
const DEFAULT_MAX = 20;

/** 一覧の行に出せる課題の属性。設定では、選んだものの key をこの順にカンマで区切って shown に持つ。 */
const FIELDS = [
  { key: 'status', label: 'ステータス' },
  { key: 'priority', label: '優先度' },
  { key: 'parent', label: '親' },
  { key: 'labels', label: 'ラベル' },
  { key: 'reporter', label: '報告者' },
  { key: 'assignee', label: '担当者' },
  { key: 'dueDate', label: '期限' },
] as const;
// 設定が無いときは、ステータスと担当者だけを出す。
const DEFAULT_FIELDS = 'assignee';
/** fields は、ステータスをいつも出し、ほかの属性だけを選んでいた形の設定なので、ステータスを足して読む。 */
const fieldsOf = (settings: WidgetSettings): Set<string> => keysOf(settings.shown, `status,${settings.fields ?? DEFAULT_FIELDS}`);
/** グループ化の属性の key をカンマで区切って持つ。groups の無い設定は、group の属性 1 つと、groupProject のときはプロジェクトでも分ける。 */
const groupKeysOf = (settings: WidgetSettings): Set<string> => keysOf(settings.groups, [settings.groupProject && 'project', settings.group].filter(Boolean).join(','));

/** JIRA のステータスカテゴリの key。new は未着手、indeterminate は進行中、done は完了。 */
const CATEGORY_TONE: Record<string, 'muted' | 'info' | 'ok' | undefined> = { new: 'muted', indeterminate: 'info', done: 'ok' };
/** JIRA の既定の優先度名。独自の優先度名は色を付けずに出す。 */
const PRIORITY_TONE: Record<string, 'bad' | 'warn' | 'info' | undefined> = { Highest: 'bad', High: 'bad', Medium: 'warn', Low: 'info', Lowest: 'info' };

/** 選んだ属性のうち、値のあるものを出す。担当者は、いないことも分かるよう未割り当てと出す。優先度と親は別の場所に出す。 */
const metaOf = (issue: JiraIssue, fields: Set<string>): string[] =>
  [
    fields.has('reporter') && issue.reporter && `報告 ${issue.reporter}`,
    fields.has('assignee') && (issue.assignee ?? '未割り当て'),
    fields.has('dueDate') && issue.dueDate && `期限 ${issue.dueDate}`,
  ].filter((v) => typeof v === 'string');

/** 行は課題そのものへのリンクで、a を入れ子にできないため、親はクリックで行のリンクを打ち消して親の課題を開く。 */
const openParent = (key: string) => (e: MouseEvent) => {
  e.preventDefault();
  floatUp(JIRA_KEY, '', { id: key });
};

export const IssueRow = ({ issue, fields }: { issue: JiraIssue; fields: Set<string> }) => (
  <li>
    <Ext class="jira-row" href={issue.url}>
      <span class="jira-head">
        <span class="jira-key">
          {fields.has('parent') && issue.parent && (
            <><span class="jira-parent" title={issue.parent.summary} onClick={openParent(issue.parent.key)}>{issue.parent.key} {issue.parent.summary}</span> / </>
          )}
          {issue.key}
        </span>
        {fields.has('status') && issue.status && <Chip tone={CATEGORY_TONE[issue.statusCategory ?? '']}>{issue.status}</Chip>}
        {fields.has('priority') && issue.priority && <Chip tone={PRIORITY_TONE[issue.priority]}>{issue.priority}</Chip>}
        {fields.has('labels') && issue.labels?.map((l) => <Chip key={l}>{l}</Chip>)}
        <span class="jira-rule" aria-hidden="true" />
        <span class="jira-meta">
          {metaOf(issue, fields).join(' · ')}
          {issue.updatedAt !== undefined && <>{metaOf(issue, fields).length > 0 && ' · '}<Time at={issue.updatedAt} /></>}
        </span>
      </span>
      <span class="jira-summary">{issue.summary}</span>
    </Ext>
  </li>
);

/** 組の見出しの名前。ステータス、優先度、エピックは、一覧の行と同じ色と装飾で出す。 */
const GroupName = ({ by, group }: { by: Grouping<JiraIssue>; group: ItemGroup<JiraIssue> }) => {
  // 組の課題はどれも同じ値を持つので、先頭の課題から色を決める。値の無い組は名前だけを出す。
  const [issue] = group.items;
  if (!issue || by.of(issue) === undefined) return <>{group.name}</>;
  if (by.key === 'status') return <Chip tone={CATEGORY_TONE[issue.statusCategory ?? '']}>{group.name}</Chip>;
  if (by.key === 'priority') return <Chip tone={PRIORITY_TONE[group.name]}>{group.name}</Chip>;
  if (by.key === 'epic' && issue.parent) {
    return <span class="jira-parent" title={issue.parent.summary} onClick={openParent(issue.parent.key)}>{group.name}</span>;
  }
  return <>{group.name}</>;
};

/** タイトルバー右端の更新のボタン。サーバーに JIRA の情報を取り直させ、JQL が同じでも探し直す。 */
export const jiraControls = (): Control[] => [
  { kind: 'refresh', label: 'JIRA の課題の情報を取り直す', onClick: () => void attempt('JIRA の更新', api.refreshJira) },
];

const JIRA_SPEC: ListSpec<JiraIssue> = {
  title: JIRA_TITLE,
  noun: '課題',
  query: { key: 'jql', label: '条件（JQL）', fallback: DEFAULT_JQL },
  maxLimit: JIRA_LIST_MAX,
  defaultMax: DEFAULT_MAX,
  fields: FIELDS,
  fieldsOf,
  groupings: JIRA_GROUPINGS,
  groupKeysOf,
  keyOf: (i) => i.key,
  textOf: (i) => [i.key, i.summary, i.status, i.priority, i.parent?.key, i.parent?.summary, i.assignee, i.reporter, ...(i.labels ?? [])],
  // エピックで分けている間は、エピックそのものは組の見出しが表すので、行には出さない。
  hide: (i, groupKeys) => groupKeys.has('epic') && Boolean(i.isEpic),
  Row: ({ item, fields }) => <IssueRow issue={item} fields={fields} />,
  GroupName,
};

export const JiraIssues = ({ snapshot, settings, onSettings }: HomeWidgetProps) => (
  <ListWidget
    spec={JIRA_SPEC}
    settings={settings}
    onSettings={onSettings}
    useFound={(jql, max) => {
      // 1 件多く探し、表示する件数を超える課題があるかを知る。
      const { data, error } = useCached(() => api.jira(jql, max + 1), [jql, max, snapshot.jiraRefreshedAt]);
      return { items: data?.slice(0, max), more: (data?.length ?? 0) > max, error };
    }}
  />
);
