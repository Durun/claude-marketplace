// JIRA Issues のウィジェットで、課題を属性の値ごとの組に分ける。
import type { JiraIssue } from '../shared/types.ts';

/** JIRA の既定の優先度名を高い順に並べたもの。独自の優先度名はこれらの後に置く。 */
const PRIORITIES = ['Highest', 'High', 'Medium', 'Low', 'Lowest'];
/** JIRA のステータスカテゴリの key を、作業の進む順に並べたもの。 */
const CATEGORIES = ['new', 'indeterminate', 'done'];

const rankIn = (list: string[], value: string | undefined): number => {
  const i = list.indexOf(value ?? '');
  return i < 0 ? list.length : i;
};

/**
 * 課題を分ける属性。of は課題の入る組の名前で、値の無い課題は undefined を返し、none の名前の組にまとめる。
 * rank は組を並べる順で、小さいほど先に置く。無ければ、課題が先に出た組から並べる。
 */
export type Grouping = { key: string; label: string; none: string; of: (issue: JiraIssue) => string | undefined; rank?: (issue: JiraIssue) => number };

/** ponytail: エピックは親の課題で代えるので、サブタスクは親のストーリーなどでまとまる。サブタスクもエピックでまとめたくなったら、親の親を読む。 */
export const GROUPINGS: Grouping[] = [
  { key: 'epic', label: 'エピック', none: 'エピックなし', of: (i) => i.parent && [i.parent.key, i.parent.summary].filter(Boolean).join(' ') },
  { key: 'status', label: 'ステータス', none: 'ステータスなし', of: (i) => i.status, rank: (i) => rankIn(CATEGORIES, i.statusCategory) },
  { key: 'dueDate', label: '期限', none: '期限なし', of: (i) => i.dueDate, rank: (i) => Date.parse(i.dueDate ?? '') },
  { key: 'assignee', label: '担当者', none: '未割り当て', of: (i) => i.assignee },
  { key: 'priority', label: '優先度', none: '優先度なし', of: (i) => i.priority, rank: (i) => rankIn(PRIORITIES, i.priority) },
  // 課題のキーの番号の前の部分がプロジェクトのキーなので、どの課題にも値がある。
  { key: 'project', label: 'プロジェクト', none: '', of: (i) => i.key.replace(/-\d+$/, '') },
];

export type IssueGroup = { name: string; issues: JiraIssue[] };

/** 課題を組に分け、値の無い課題の組は最後に置く。組の中の課題は、渡した順のまま並べる。 */
export const groupIssues = (issues: JiraIssue[], by: Grouping): IssueGroup[] => {
  // 課題を組の順に並べてから分けると、組も同じ順に並ぶ。値の無い課題どうしの差は NaN になるので、0 として扱う。
  const rank = (i: JiraIssue): number => (by.of(i) === undefined ? Infinity : (by.rank?.(i) ?? 0));
  const sorted = issues.toSorted((a, b) => rank(a) - rank(b) || 0);
  return Array.from(Map.groupBy(sorted, by.of), ([name, list]) => ({ name: name ?? by.none, issues: list }));
};
