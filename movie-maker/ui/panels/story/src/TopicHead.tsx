import { useState } from "preact/hooks";
import type { Rating, Topic } from "@mm/sdk";
import { formatClock, toSeconds } from "./model.ts";

const RATINGS: { rating: Rating; label: string; title: string }[] = [
  { rating: "favorite", label: "Use", title: "Favorite" },
  { rating: "rejected", label: "Skip", title: "Rejected" },
];

export interface TopicHeadProps {
  topic: Topic;
  picked: boolean;
  onPick: () => void;
  onRate: (rating: Rating) => void;
  onTag: (tag: string, remove: boolean) => void;
  onRemove: () => void;
}

/** 新しいタグの入力。Enter で付ける。 */
const TagInput = ({ onAdd }: { onAdd: (tag: string) => void }) => {
  const [draft, setDraft] = useState("");
  const submit = (e: Event) => {
    e.preventDefault();
    const tag = draft.trim();
    if (tag) onAdd(tag);
    setDraft("");
  };
  return (
    <form class="mm-story-tag-form" onSubmit={submit}>
      <input
        class="mm-story-input mm-story-tag-input"
        aria-label="Add tag"
        placeholder="+ tag"
        value={draft}
        onInput={(e) => setDraft(e.currentTarget.value)}
      />
    </form>
  );
};

/** 話題の見出し。題、時間、評価、タグ、並べる対象にするチェック。 */
export const TopicHead = ({ topic, picked, onPick, onRate, onTag, onRemove }: TopicHeadProps) => {
  const rating = topic.rating ?? "unrated";
  const span = `${formatClock(toSeconds(topic.in))} - ${formatClock(toSeconds(topic.out))}`;
  return (
    <header class="mm-story-topic-head" data-rating={rating}>
      <label class="mm-story-pick">
        <input type="checkbox" checked={picked} onChange={onPick} aria-label="Pick topic" />
        <span class="mm-story-topic-title">{topic.title}</span>
      </label>
      <span class="mm-story-time">{span}</span>
      <div class="mm-story-topic-tools">
        {RATINGS.map((r) => (
          <button
            key={r.rating}
            type="button"
            class="mm-story-chip"
            title={r.title}
            aria-pressed={rating === r.rating}
            onClick={() => onRate(r.rating)}
          >
            {r.label}
          </button>
        ))}
        <button type="button" class="mm-story-chip" title="Remove topic" onClick={onRemove}>
          Remove
        </button>
        {(topic.tags ?? []).map((tag) => (
          <button
            key={tag}
            type="button"
            class="mm-story-tag"
            title="Remove tag"
            onClick={() => onTag(tag, true)}
          >
            {tag}
          </button>
        ))}
        <TagInput onAdd={(tag) => onTag(tag, false)} />
      </div>
    </header>
  );
};
