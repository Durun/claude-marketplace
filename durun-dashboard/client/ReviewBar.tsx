import { useState } from 'preact/hooks';
import type { PrInfo, ReviewComment, Session } from '../shared/types.ts';
import { DELIVERY_TOAST, api, attempt } from './api.ts';
import type { ReviewDraft } from './review.ts';
import { toast } from './toast.tsx';

const APPROVE_SUMMARY = '指摘なし';

const inFileOrder = (list: ReviewComment[]): ReviewComment[] =>
  [...list].sort((a, b) => a.path.localeCompare(b.path) || a.line - b.line);

type Props = {
  s: Session;
  review: ReviewDraft;
  /** ブランチを表示してコメントを付けているとき。どのブランチの行かを Claude に添える。 */
  branch?: string;
  /** PR を表示しているときの PR と、行番号を数えた PR の先端。送り先に GitHub を選べる。 */
  pr?: { info: PrInfo; commit?: string; onPosted: () => void };
};

export const ReviewBar = ({ s, review, branch, pr }: Props) => {
  const [sending, setSending] = useState(false);
  const [toGitHub, setToGitHub] = useState(false);
  const count = review.comments.length;
  const github = toGitHub && pr;
  const submit = async (comments: ReviewComment[], summary: string) => {
    setSending(true);
    const res = await attempt('レビューの送信', () => api.review(s.id, inFileOrder(comments), summary.trim(), branch));
    setSending(false);
    if (!res) return;
    toast(DELIVERY_TOAST[res.delivery]);
    review.clear();
  };
  const post = async () => {
    if (!github || !github.commit) return;
    setSending(true);
    const res = await attempt('PR への投稿', () => api.prReview(s.id, inFileOrder(review.comments), review.summary.trim(), github.commit ?? ''));
    setSending(false);
    if (!res) return;
    toast(`PR #${github.info.number} に投稿しました`);
    review.clear();
    github.onPosted();
  };
  // 承認は下書きのコメントを捨てて送るため、残っているときは確認する。
  const approve = () => {
    if (count > 0 && !confirm(`下書きのコメント ${count} 件は送らずに破棄されます。指摘なしで承認しますか?`)) return;
    submit([], APPROVE_SUMMARY);
  };
  return (
    <div class="review-bar">
      {s.awaitingReview && (
        <div class="review-banner">
          <span>Claude がレビューを待っています</span>
          <button class="btn btn-sm btn-primary" disabled={sending} onClick={approve}>指摘なしで承認</button>
        </div>
      )}
      <div class="review-controls">
        <span class="review-count">
          <span>コメント <b>{count}</b> 件</span>
          {pr && (
            <select class="input input-sm" aria-label="送り先" value={toGitHub ? 'github' : 'claude'} onChange={(e) => setToGitHub(e.currentTarget.value === 'github')}>
              <option value="claude">Claude に送る</option>
              <option value="github">PR #{pr.info.number} に投稿</option>
            </select>
          )}
        </span>
        <textarea
          class="input"
          rows={2}
          placeholder="全体コメント（任意）"
          aria-label="全体コメント"
          value={review.summary}
          onInput={(e) => review.setSummary(e.currentTarget.value)}
        />
        <button
          class="btn btn-primary"
          disabled={sending || (count === 0 && !review.summary.trim()) || (github && !github.commit)}
          onClick={() => (github ? post() : submit(review.comments, review.summary))}
        >
          {github ? 'GitHub に投稿' : 'レビューを送信'}
        </button>
      </div>
    </div>
  );
};
