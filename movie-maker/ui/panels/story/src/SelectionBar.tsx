import { useState } from "preact/hooks";
import type { StoryWord } from "@mm/sdk";
import { defaultTitle, formatClock, shownText, toSeconds } from "./model.ts";

const TITLE_MAX_CHARS = 16;

export interface SelectionBarProps {
  selected: readonly StoryWord[];
  onTopic: (title: string) => void;
  onDelete: () => void;
  onFix: (text: string) => void;
  onClear: () => void;
}

/** 直す文字の入力。1 語を選んだときだけ出す。空にして Enter で直しを外す。 */
const FixForm = ({ word, onFix }: { word: StoryWord; onFix: (text: string) => void }) => {
  const [draft, setDraft] = useState(shownText(word));
  return (
    <form
      class="mm-story-row"
      onSubmit={(e) => {
        e.preventDefault();
        onFix(draft.trim());
      }}
    >
      <input
        class="mm-story-input"
        aria-label="Fixed text"
        value={draft}
        onInput={(e) => setDraft(e.currentTarget.value)}
      />
      <button type="submit" class="mm-story-button">
        Fix
      </button>
    </form>
  );
};

/** 選んだ単語への操作。話題にする、削除（戻す）、直す。 */
export const SelectionBar = ({ selected, onTopic, onDelete, onFix, onClear }: SelectionBarProps) => {
  const [title, setTitle] = useState("");
  const first = selected[0];
  const last = selected.at(-1);
  if (!first || !last) return null;
  const restore = selected.every((w) => w.deleted);
  const span = `${formatClock(toSeconds(first.start))} - ${formatClock(toSeconds(last.end))}`;
  const topic = () => {
    onTopic(title.trim() || defaultTitle(selected, TITLE_MAX_CHARS));
    setTitle("");
  };
  return (
    <div class="mm-story-bar" role="toolbar" aria-label="Selection">
      <div class="mm-story-row">
        <span class="mm-story-meta">
          {selected.length} words, {span}
        </span>
        <button type="button" class="mm-story-link" onClick={onClear}>
          Clear
        </button>
      </div>
      <div class="mm-story-row">
        <input
          class="mm-story-input"
          aria-label="Topic title"
          placeholder="Topic title"
          value={title}
          onInput={(e) => setTitle(e.currentTarget.value)}
        />
        <button type="button" class="mm-story-button mm-story-primary" onClick={topic}>
          Make topic
        </button>
        <button type="button" class="mm-story-button" onClick={onDelete}>
          {restore ? "Restore" : "Delete"}
        </button>
      </div>
      {selected.length === 1 && <FixForm key={first.id} word={first} onFix={onFix} />}
    </div>
  );
};
