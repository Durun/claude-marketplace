import { useState } from "preact/hooks";
import type { ReviewApi } from "./api.ts";
import { DecidedCard, PendingCard } from "./ProposalCard.tsx";
import { useReview } from "./useReview.ts";

export interface ReviewPanelProps {
  api: ReviewApi;
  /** 増えるたびに一覧を読み直す（`mm:doc-changed` が届いたとき）。 */
  refreshKey?: number;
  /** 見えているか。隠れている間は Program を元のフレームに戻す。 */
  active?: boolean;
  /** 相対時刻の基準（ミリ秒）。無ければ現在時刻。スクリーンショットとテストで固定する。 */
  nowMs?: number;
}

/** Claude が出した編集の提案を、承認、却下する。保留中が上、決定済みが下（畳める）。 */
export const ReviewPanel = ({ api, refreshKey = 0, active = true, nowMs }: ReviewPanelProps) => {
  const review = useReview(api, refreshKey, active);
  const [showDecided, setShowDecided] = useState(true);
  const now = nowMs ?? Date.now();
  const pending = review.proposals?.filter((p) => p.state === "pending") ?? [];
  const decided = review.proposals?.filter((p) => p.state !== "pending") ?? [];
  return (
    <section class="mm-review" aria-label="Review">
      {review.error && (
        <p class="mm-review-error" role="alert">
          {review.error}
        </p>
      )}
      {review.proposals && review.proposals.length === 0 && <p class="mm-review-empty">No proposals from Claude yet.</p>}
      {pending.length > 0 && (
        <ul class="mm-review-list" aria-label="Pending proposals">
          {pending.map((p) => (
            <PendingCard key={p.id} proposal={p} review={review} nowMs={now} />
          ))}
        </ul>
      )}
      {decided.length > 0 && (
        <div class="mm-review-decided">
          <button type="button" class="mm-review-fold" aria-expanded={showDecided} onClick={() => setShowDecided(!showDecided)}>
            <span class="mm-review-chevron" aria-hidden="true" />
            Decided ({decided.length})
          </button>
          {showDecided && (
            <ul class="mm-review-list" aria-label="Decided proposals">
              {decided.map((p) => (
                <DecidedCard key={p.id} proposal={p} review={review} nowMs={now} />
              ))}
            </ul>
          )}
        </div>
      )}
    </section>
  );
};
