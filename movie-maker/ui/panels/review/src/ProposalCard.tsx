import type { Proposal } from "./api.ts";
import { approveLabel, describeOp, describeScope, relativeTime } from "./model.ts";
import { checkedIndexes, type ReviewState } from "./useReview.ts";

interface CardProps {
  proposal: Proposal;
  review: ReviewState;
  nowMs: number;
}

interface PartProps {
  p: Proposal;
  review: ReviewState;
}

const Header = ({ p, nowMs }: { p: Proposal; nowMs: number }) => (
  <header class="mm-review-head">
    <h3 class="mm-review-name">{p.name}</h3>
    <div class="mm-review-meta">
      {p.actor_name} via {p.actor} · {relativeTime(p.at_ms, nowMs)}
      {p.responds_to && <> · re: comment {p.responds_to}</>}
    </div>
  </header>
);

const Scope = ({ p }: { p: Proposal }) => (
  <ul class="mm-review-scope" aria-label="Scope">
    {describeScope(p.scope).map((line) => (
      <li key={line}>{line}</li>
    ))}
  </ul>
);

const PreviewToggle = ({ p, review }: PartProps) => {
  const proposed = review.previewId === p.id;
  return (
    <div class="mm-review-toggle" role="group" aria-label="Preview">
      <button type="button" class="mm-review-seg" aria-pressed={!proposed} onClick={() => review.setPreviewing(p, false)}>
        Original
      </button>
      <button type="button" class="mm-review-seg" aria-pressed={proposed} onClick={() => review.setPreviewing(p, true)}>
        Proposed
      </button>
    </div>
  );
};

const PendingOps = ({ p, review }: PartProps) => {
  const checked = checkedIndexes(p, review.selections);
  return (
    <ul class="mm-review-ops" aria-label="Operations">
      {p.ops.map((o) => (
        <li key={o.index}>
          <label class="mm-review-op">
            <input type="checkbox" checked={checked.includes(o.index)} onChange={() => review.toggleOp(p, o.index)} />
            <span>{describeOp(o)}</span>
          </label>
        </li>
      ))}
    </ul>
  );
};

/** 保留中の提案。操作ごとにチェックを持ち、チェックした分だけ承認できる。 */
export const PendingCard = ({ proposal: p, review, nowMs }: CardProps) => {
  const selected = checkedIndexes(p, review.selections).length;
  const conflict = p.conflict && p.conflict.length > 0 ? p.conflict : null;
  return (
    <li class="mm-review-card" data-state="pending">
      <Header p={p} nowMs={nowMs} />
      <Scope p={p} />
      <PendingOps p={p} review={review} />
      {conflict && (
        <p class="mm-review-conflict" role="alert">
          Conflicts with later edits: {conflict.join(", ")}
        </p>
      )}
      <div class="mm-review-actions">
        <PreviewToggle p={p} review={review} />
        <button type="button" class="mm-review-button" onClick={() => review.reject(p)}>
          Reject
        </button>
        <button
          type="button"
          class="mm-review-button mm-review-primary"
          disabled={conflict !== null || selected === 0}
          onClick={() => review.approve(p)}
        >
          {approveLabel(selected, p.ops.length)}
        </button>
      </div>
    </li>
  );
};

const decidedLabel = (p: Proposal): string => {
  if (p.state === "rejected") return "Rejected";
  return p.applied ? "Applied" : "Undone";
};

/** 決定済みの提案。承認したものは取り消せるときだけ Undo、却下したものは Reopen。 */
export const DecidedCard = ({ proposal: p, review, nowMs }: CardProps) => (
  <li class="mm-review-card" data-state={p.state}>
    <Header p={p} nowMs={nowMs} />
    <Scope p={p} />
    <ul class="mm-review-ops" aria-label="Operations">
      {p.ops.map((o) => (
        <li key={o.index} class="mm-review-op-plain" data-skipped={p.approved_ops !== null && !p.approved_ops.includes(o.index)}>
          {describeOp(o)}
        </li>
      ))}
    </ul>
    <div class="mm-review-actions">
      <span class="mm-review-status" data-status={decidedLabel(p)}>
        {decidedLabel(p)}
      </span>
      {p.state === "approved" && p.undoable && (
        <button type="button" class="mm-review-button" onClick={() => review.undo(p)}>
          Undo
        </button>
      )}
      {p.state === "rejected" && (
        <button type="button" class="mm-review-button" onClick={() => review.reopen(p)}>
          Reopen
        </button>
      )}
    </div>
  </li>
);
