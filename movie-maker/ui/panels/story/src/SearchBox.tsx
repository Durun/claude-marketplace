import { useState } from "preact/hooks";
import type { StoryAsset, StoryHit } from "./api.ts";
import { formatClock, toSeconds } from "./model.ts";

export interface SearchBoxProps {
  hits: StoryHit[] | null;
  assets: readonly StoryAsset[];
  onSearch: (query: string) => void;
  onClear: () => void;
  onOpen: (hit: StoryHit) => void;
}

/** 検索の入力と結果。Enter で探す。結果を押すとその単語へ移る。 */
export const SearchBox = ({ hits, assets, onSearch, onClear, onOpen }: SearchBoxProps) => {
  const [query, setQuery] = useState("");
  const nameOf = (id: string) => assets.find((a) => a.id === id)?.name ?? id;
  return (
    <div class="mm-story-search">
      <form
        class="mm-story-row"
        role="search"
        onSubmit={(e) => {
          e.preventDefault();
          onSearch(query);
        }}
      >
        <input
          class="mm-story-input"
          type="search"
          aria-label="Search transcript"
          placeholder="Search transcript"
          value={query}
          onInput={(e) => {
            setQuery(e.currentTarget.value);
            if (e.currentTarget.value === "") onClear();
          }}
        />
        <button type="submit" class="mm-story-button">
          Search
        </button>
      </form>
      {hits && (
        <ul class="mm-story-hits" aria-label="Search results">
          {hits.length === 0 && <li class="mm-story-meta">No matches.</li>}
          {hits.map((hit) => (
            <li key={`${hit.asset}-${hit.word}`}>
              <button type="button" class="mm-story-hit" onClick={() => onOpen(hit)}>
                <span class="mm-story-time">{formatClock(toSeconds(hit.start))}</span>
                <span class="mm-story-hit-context">{hit.context}</span>
                {assets.length > 1 && <span class="mm-story-meta">{nameOf(hit.asset)}</span>}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
};
