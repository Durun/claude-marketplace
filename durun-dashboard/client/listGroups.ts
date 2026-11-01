// JIRA Issues と GitHub Inbox のウィジェットで、一覧の項目を属性の値ごとの組に分ける。
import type { GithubItem, JiraIssue } from '../shared/types.ts';

/** 項目を分ける属性。of は項目の入る組の名前で、値の無い項目は undefined を返し、none の名前の組にまとめる。 */
export type Grouping<T> = { key: string; label: string; none: string; of: (item: T) => string | undefined };

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
  { key: 'section', label: 'Inbox の区分', none: 'Other', of: (i) => i.section },
  { key: 'repo', label: 'リポジトリ', none: '', of: (i) => i.repo },
  { key: 'author', label: '作成者', none: '', of: (i) => i.author },
  { key: 'review', label: 'レビューの判定', none: 'レビュー不要', of: (i) => i.review && REVIEW_LABEL[i.review] },
  { key: 'checks', label: 'チェックの状態', none: 'チェックなし', of: (i) => i.checks && CHECKS_LABEL[i.checks] },
  { key: 'draft', label: 'ドラフト', none: 'ドラフトでない', of: (i) => (i.isDraft ? 'ドラフト' : undefined) },
  { key: 'kind', label: 'PR/Issue', none: '', of: (i) => (i.kind === 'pr' ? 'PR' : 'Issue') },
];

/**
 * 選んだ属性を、結果の並びでまとまっているものから外側に並べる。
 * 検索の条件で先に並べた属性ほど、同じ値の項目が続く。値が切り替わる回数から値の種類の数を引いた数が少ないほど、まとまっているとみなす。同じ数なら groupings の順に置く。
 */
export const groupingsFor = <T,>(groupings: Grouping<T>[], keys: Set<string>, items: T[]): Grouping<T>[] => {
  const scattered = (g: Grouping<T>): number => {
    const values = items.map(g.of);
    return values.filter((v, i) => i === 0 || v !== values[i - 1]).length - new Set(values).size;
  };
  return groupings.filter((g) => keys.has(g.key))
    .map((g) => ({ g, n: scattered(g) }))
    .sort((a, b) => a.n - b.n)
    .map(({ g }) => g);
};

export type ItemGroup<T> = { name: string; items: T[] };

/** 項目を、渡した順のまま、同じ値が続くところで組にまとめる。並びは検索の条件の並べ方に任せ、離れた項目を同じ組へ寄せない。 */
export const groupItems = <T,>(items: T[], by: Grouping<T>): ItemGroup<T>[] => {
  const groups: ItemGroup<T>[] = [];
  for (const item of items) {
    const name = by.of(item) ?? by.none;
    const last = groups.at(-1);
    if (last?.name === name) last.items.push(item);
    else groups.push({ name, items: [item] });
  }
  return groups;
};
