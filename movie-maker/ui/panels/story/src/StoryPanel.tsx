import { useState } from "preact/hooks";
import type { StoryApi } from "./api.ts";
import { ArrangeBar, arrangeTarget } from "./ArrangeBar.tsx";
import { SearchBox } from "./SearchBox.tsx";
import { SelectionBar } from "./SelectionBar.tsx";
import { TranscriptView, type TopicActions } from "./TranscriptView.tsx";
import { wordsIn } from "./model.ts";
import { createActions, type Actions } from "./useActions.ts";
import { useDragSelect } from "./useDragSelect.ts";
import { useLoaded, useSearch, useTray, type Tray } from "./useStory.ts";

export interface StoryPanelProps {
  api: StoryApi;
  /** 増えるたびに、素材と文字起こしを読み直す（タブを開いたとき、外から編集されたとき）。 */
  refreshKey?: number;
}

/** 素材の選び方。文字起こしがある素材だけを並べる。 */
const AssetPicker = ({ loaded }: { loaded: ReturnType<typeof useLoaded> }) => {
  const assets = (loaded.overview?.assets ?? []).filter((a) => a.has_transcript);
  return (
    <select
      class="mm-story-input"
      aria-label="Clip"
      value={loaded.assetId ?? ""}
      disabled={assets.length === 0}
      onChange={(e) => loaded.selectAsset(e.currentTarget.value)}
    >
      {assets.map((a) => (
        <option key={a.id} value={a.id}>
          {a.name}
        </option>
      ))}
    </select>
  );
};

const Message = ({ error, notice }: { error: string | null; notice: string | null }) => {
  const text = error ?? notice;
  return text ? <p class={error ? "mm-story-error" : "mm-story-meta"}>{text}</p> : null;
};

const topicActions = (actions: Actions, tray: Tray): TopicActions => ({
  pickedIds: new Set(tray.picked.map((t) => t.id)),
  onPick: tray.toggle,
  onRate: actions.rate,
  onTag: actions.tag,
  onRemove: actions.removeTopic,
});

/** 並べ先の sequence と、Delete が切る sequence（開いているコンポジションの sequence、無ければ並べ先）。 */
const useSequenceTargets = (loaded: ReturnType<typeof useLoaded>) => {
  const [chosen, choose] = useState<string | null>(null);
  const sequences = loaded.overview?.sequences ?? [];
  const target = arrangeTarget(chosen, sequences);
  return { sequences, target, choose, cutSequence: loaded.overview?.open_sequence ?? target };
};

/** 文字起こしを読み物として見せ、範囲を選んで話題にする・消す・直す・並べる。 */
export const StoryPanel = ({ api, refreshKey = 0 }: StoryPanelProps) => {
  const loaded = useLoaded(api, refreshKey);
  const tray = useTray();
  const search = useSearch(api, loaded.setError);
  const [focusWord, setFocusWord] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const words = loaded.transcript?.words ?? [];
  const select = useDragSelect((i) => {
    const word = words[i];
    if (word) void actions.jump(word);
  }, words.length);
  const selected = select.dragging ? [] : wordsIn(words, select.span);
  const { cutSequence, ...seq } = useSequenceTargets(loaded);
  const actions = createActions({ api, loaded, tray, selected, cutSequence, clearSelection: select.clear, setFocusWord, setNotice });
  const assets = loaded.overview?.assets ?? [];

  return (
    <section class="mm-story" aria-label="Story">
      <AssetPicker loaded={loaded} />
      <SearchBox hits={search.hits} assets={assets} onSearch={search.run} onClear={search.clear} onOpen={actions.openHit} />
      <Message error={loaded.error} notice={notice} />
      <div class="mm-story-scroll">
        {loaded.transcript && words.length > 0 ? (
          <TranscriptView
            words={words}
            topics={loaded.transcript.topics}
            select={select}
            focusWord={focusWord}
            actions={topicActions(actions, tray)}
          />
        ) : (
          <p class="mm-story-empty">No transcript yet. Transcribe a clip with tools/transcribe, then reopen this tab.</p>
        )}
      </div>
      <SelectionBar
        selected={selected}
        onTopic={actions.makeTopic}
        onCut={actions.cutSelected}
        onHide={actions.hideSelected}
        onFix={actions.fixSelected}
        onClear={select.clear}
      />
      <ArrangeBar tray={tray} {...seq} onChoose={seq.choose} onArrange={actions.arrange} />
    </section>
  );
};
