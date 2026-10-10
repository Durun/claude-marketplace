import { useState } from 'preact/hooks';
import type { DiffLine, ReviewComment } from '../shared/types.ts';

type Draft = { comments: ReviewComment[]; summary: string };

const EMPTY: Draft = { comments: [], summary: '' };

// ブランチを表示して付けたコメントは、行番号がそのブランチのものなので、下書きをブランチごとに分ける。
const storageKey = (sessionId: string, branch?: string): string => (branch ? `review:${sessionId}:${branch}` : `review:${sessionId}`);

const load = (key: string): Draft => {
  try {
    const raw = localStorage.getItem(key);
    return raw ? { ...EMPTY, ...(JSON.parse(raw) as Partial<Draft>) } : EMPTY;
  } catch {
    return EMPTY;
  }
};

const save = (key: string, draft: Draft): void => {
  try {
    if (draft.comments.length === 0 && !draft.summary) localStorage.removeItem(key);
    else localStorage.setItem(key, JSON.stringify(draft));
  } catch {
    // 保存できなくても、画面上の下書きはそのまま使える。
  }
};

type CommentTarget = Pick<ReviewComment, 'path' | 'side' | 'line'>;

export const commentKey = (c: CommentTarget): string => `${c.path}\n${c.side}:${c.line}`;

/** 追加行と文脈行は新しい側の行番号、削除行は古い側の行番号でコメントを付ける。 */
export const lineTarget = (l: DiffLine): Pick<ReviewComment, 'side' | 'line'> =>
  l.newNo != null ? { side: 'new', line: l.newNo } : { side: 'old', line: l.oldNo ?? 0 };

export type ReviewDraft = ReturnType<typeof useReviewDraft>;

export const useReviewDraft = (sessionId: string, branch?: string) => {
  const key = storageKey(sessionId, branch);
  const [draft, setDraft] = useState<Draft>(() => load(key));
  const update = (next: Draft) => {
    setDraft(next);
    save(key, next);
  };
  return {
    comments: draft.comments,
    summary: draft.summary,
    setSummary: (summary: string) => update({ ...draft, summary }),
    upsert: (c: ReviewComment) =>
      update({ ...draft, comments: [...draft.comments.filter((x) => commentKey(x) !== commentKey(c)), c] }),
    remove: (key: string) => update({ ...draft, comments: draft.comments.filter((c) => commentKey(c) !== key) }),
    clear: () => update(EMPTY),
  };
};
