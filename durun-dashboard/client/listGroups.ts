// JIRA Issues と GitHub Inbox のウィジェットで、一覧の項目を属性の値ごとの組に分ける。
import type { GithubItem, JiraIssue } from '../shared/types.ts';

/** 項目を分ける属性。of は項目の入る組の名前で、値の無い項目は undefined を返し、none の名前の組にまとめる。order は組を並べる名前の順。 */
export type Grouping<T> = { key: string; label: string; none: string; of: (item: T) => string | undefined; order?: string[] };

/**
 * エピックの組には、エピックそのものを入れない。組の見出しがエピックを表す。
 * ponytail: エピックは親の課題で代えるので、サブタスクは親のストーリーなどでまとまる。サブタスクもエピックでまとめたくなったら、親の親を読む。
 */
export const JIRA_GROUPINGS: Grouping<JiraIssue>[] = [
  { key: 'epic', label: 'エピック', none: 'エピックなし', of: (i) => i.parent && [i.parent.key, i.parent.summary].filter(Boolean).join(' ') },
  { key: 'status', label: 'ステータス', none: 'ステータスなし', of: (i) => i.status },
  { key: 'dueDate', label: '期限', none: '期限なし', of: (i) => i.dueDate && `期限 ${i.dueDate}` },
  { key: 'assignee', label: '担当者', none: '未割り当て', of: (i) => i.assignee },
  { key: 'priority', label: '優先度', none: '優先度なし', of: (i) => i.priority },
  // 課題のキーの番号の前の部分がプロジェクトのキーなので、どの課題にも値がある。
  { key: 'project', label: 'プロジェクト', none: '', of: (i) => i.key.replace(/-\d+$/, '') },
];

/** PR のレビューの判定と、組や行に出す名前。 */
export const REVIEW_LABEL: Record<string, string> = { APPROVED: '承認済み', CHANGES_REQUESTED: '修正依頼', REVIEW_REQUIRED: 'レビュー待ち' };
/** チェックの状態と、組や行に出す名前。 */
export const CHECKS_LABEL: Record<string, string> = { SUCCESS: 'チェック成功', FAILURE: 'チェック失敗', ERROR: 'チェック失敗', PENDING: 'チェック中', EXPECTED: 'チェック中' };

export const GITHUB_GROUPINGS: Grouping<GithubItem>[] = [
  // GitHub の PR のダッシュボードの Inbox と同じ順に並べる。
  {
    key: 'section',
    label: 'Inbox の区分',
    none: 'Other',
    of: (i) => i.section,
    order: ['Needs your review', "Needs your teams' review", 'Your drafts', 'Waiting for review or checks', 'Needs action', 'Ready to merge'],
  },
  { key: 'repo', label: 'リポジトリ', none: '', of: (i) => i.repo },
  { key: 'author', label: '作成者', none: '', of: (i) => i.author },
  { key: 'review', label: 'レビューの判定', none: 'レビュー不要', of: (i) => i.review && REVIEW_LABEL[i.review] },
  { key: 'checks', label: 'チェックの状態', none: 'チェックなし', of: (i) => i.checks && CHECKS_LABEL[i.checks] },
  // PR か Issue かと、その状態を 1 つにまとめた種類。
  { key: 'type', label: 'タイプ', none: '', of: (i) => `${i.state} ${i.kind === 'pr' ? 'PR' : 'Issue'}` },
];

export type ItemGroup<T> = { name: string; items: T[] };

/** 項目を、同じ値ごとに 1 つの組にまとめる。組は値が先に出た順に並べ、order のある属性はその名前の順に並べ替える。組の中の項目は、渡した順のまま残す。 */
export const groupItems = <T,>(items: T[], by: Grouping<T>): ItemGroup<T>[] => {
  const groups = Array.from(Map.groupBy(items, (item) => by.of(item) ?? by.none), ([name, list]) => ({ name, items: list }));
  const { order } = by;
  if (!order) return groups;
  // order に無い名前と値の無い組は後ろに置く。sort は安定なので、それらは先に出た順のまま残る。
  const rank = (name: string): number => {
    const i = order.indexOf(name);
    return i < 0 ? Infinity : i;
  };
  return groups.sort((x, y) => rank(x.name) - rank(y.name) || 0);
};
