import { useState } from "preact/hooks";
import type { StorySequence } from "@mm/sdk";
import type { Tray } from "./useStory.ts";

export interface ArrangeBarProps {
  tray: Tray;
  sequences: readonly StorySequence[];
  onArrange: (sequence: string) => void;
}

/** 並べる対象にした話題の順と、並べ先の sequence。 */
export const ArrangeBar = ({ tray, sequences, onArrange }: ArrangeBarProps) => {
  const [chosen, setChosen] = useState<string | null>(null);
  if (tray.picked.length === 0) return null;
  const target = chosen ?? sequences[0]?.id ?? null;
  return (
    <div class="mm-story-bar" role="region" aria-label="Arrange">
      <ol class="mm-story-tray">
        {tray.picked.map((topic, i) => (
          <li key={topic.id} class="mm-story-tray-item">
            <span class="mm-story-meta">{i + 1}</span>
            <span class="mm-story-tray-title">{topic.title}</span>
            <button type="button" class="mm-story-link" aria-label="Move up" onClick={() => tray.move(i, -1)}>
              Up
            </button>
            <button type="button" class="mm-story-link" aria-label="Move down" onClick={() => tray.move(i, 1)}>
              Down
            </button>
            <button type="button" class="mm-story-link" aria-label="Unpick" onClick={() => tray.toggle(topic)}>
              Remove
            </button>
          </li>
        ))}
      </ol>
      <div class="mm-story-row">
        <select
          class="mm-story-input"
          aria-label="Sequence"
          value={target ?? ""}
          disabled={sequences.length === 0}
          onChange={(e) => setChosen(e.currentTarget.value)}
        >
          {sequences.map((s) => (
            <option key={s.id} value={s.id}>
              {s.id} ({s.cuts} cuts)
            </option>
          ))}
        </select>
        <button
          type="button"
          class="mm-story-button mm-story-primary"
          disabled={target === null}
          onClick={() => target && onArrange(target)}
        >
          Arrange
        </button>
      </div>
      {sequences.length === 0 && <p class="mm-story-meta">No sequence in this project.</p>}
    </div>
  );
};
