// JIRA Issues のウィジェットで、課題を属性の値ごとの組に分ける。
import type { JiraIssue } from '../shared/types.ts';

/** 課題を分ける属性。of は課題の入る組の名前で、値の無い課題は undefined を返し、none の名前の組にまとめる。 */
export type Grouping = { key: string; label: string; none: string; of: (issue: JiraIssue) => string | undefined };

/**
 * エピックの組には、エピックそのものを入れない。組の見出しがエピックを表す。
 * ponytail: エピックは親の課題で代えるので、サブタスクは親のストーリーなどでまとまる。サブタスクもエピックでまとめたくなったら、親の親を読む。
 */
export const GROUPINGS: Grouping[] = [
  { key: 'epic', label: 'エピック', none: 'エピックなし', of: (i) => i.parent && [i.parent.key, i.parent.summary].filter(Boolean).join(' ') },
  { key: 'status', label: 'ステータス', none: 'ステータスなし', of: (i) => i.status },
  { key: 'dueDate', label: '期限', none: '期限なし', of: (i) => i.dueDate },
  { key: 'assignee', label: '担当者', none: '未割り当て', of: (i) => i.assignee },
  { key: 'priority', label: '優先度', none: '優先度なし', of: (i) => i.priority },
  // 課題のキーの番号の前の部分がプロジェクトのキーなので、どの課題にも値がある。
  { key: 'project', label: 'プロジェクト', none: '', of: (i) => i.key.replace(/-\d+$/, '') },
];

/** JQL の ORDER BY に書ける項目の名前と、その項目で分ける属性。 */
const ORDER_FIELDS: Record<string, string> = {
  parent: 'epic',
  'epic link': 'epic',
  status: 'status',
  statuscategory: 'status',
  duedate: 'dueDate',
  due: 'dueDate',
  assignee: 'assignee',
  priority: 'priority',
  project: 'project',
};

/** 選んだ属性を、JQL の ORDER BY に並べた順に外側から並べる。ORDER BY に無い属性は、その後に GROUPINGS の順で置く。 */
export const groupingsFor = (keys: Set<string>, jql: string): Grouping[] => {
  const order = (/\border\s+by\s+(.+)$/is.exec(jql)?.[1] ?? '')
    .split(',')
    .map((f) => ORDER_FIELDS[f.trim().replace(/\s+(asc|desc)$/i, '').replace(/^"|"$/g, '').toLowerCase()]);
  const rank = (key: string): number => {
    const i = order.indexOf(key);
    return i < 0 ? order.length : i;
  };
  return GROUPINGS.filter((g) => keys.has(g.key)).sort((a, b) => rank(a.key) - rank(b.key));
};

export type IssueGroup = { name: string; issues: JiraIssue[] };

/** 課題を組に分ける。JQL の ORDER BY に従うよう、組も組の中の課題も、渡した順で先に出たものから並べる。 */
export const groupIssues = (issues: JiraIssue[], by: Grouping): IssueGroup[] =>
  Array.from(Map.groupBy(issues, by.of), ([name, list]) => ({ name: name ?? by.none, issues: list }));
