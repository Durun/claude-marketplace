import { useState } from "preact/hooks";
import { announceAsset } from "@mm/sdk";
import type { MediaApi } from "./api.ts";
import { CacheSection } from "./CacheSection.tsx";
import { Failures, PendingRow, Row } from "./Rows.tsx";
import { useMedia } from "./useMedia.ts";
import { Waveform } from "./Waveform.tsx";

export interface MediaPanelProps {
  api: MediaApi;
  /** 外から「ファイルを選んで取り込む」を始めたいときに増やす（Tasks の項目から）。 */
  importRequest?: number;
  /** 増えるたびに素材の一覧を読み直す（外から編集されたとき）。 */
  refreshKey?: number;
}

/** 取り込みのボタン。取り込み中は押せない。 */
const Toolbar = ({ running, onImport }: { running: boolean; onImport: () => void }) => (
  <div class="mm-media-toolbar">
    <button
      type="button"
      class="mm-media-button mm-media-primary"
      disabled={running}
      onClick={onImport}
    >
      Import…
    </button>
    <span class="mm-media-hint">or drop files here</span>
  </div>
);

/** XP のムービーメーカーのコレクション風の素材の一覧。取り込み、波形、キャッシュの掃除を持つ。 */
export const MediaPanel = ({ api, importRequest = 0, refreshKey = 0 }: MediaPanelProps) => {
  const media = useMedia(api, importRequest, refreshKey);
  const { items, progress } = media;
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selected = items.find((i) => i.id === selectedId) ?? null;

  return (
    <section class="mm-media" aria-label="Media" data-dragging={media.dragging}>
      <Toolbar running={progress.running} onImport={media.pickAndImport} />
      {media.error && <p class="mm-media-error">{media.error}</p>}
      {progress.failures.length > 0 && !progress.running && (
        <Failures failures={progress.failures} onDismiss={media.dismissFailures} />
      )}
      <ul class="mm-media-list" aria-label="Clips">
        {progress.running && progress.current && <PendingRow progress={progress} />}
        {items.map((item) => (
          <Row
            key={item.id}
            api={api}
            item={item}
            selected={item.id === selectedId}
            onSelect={() => {
              setSelectedId(item.id);
              announceAsset(item.id);
            }}
          />
        ))}
        {items.length === 0 && !progress.running && (
          <li class="mm-media-empty">No clips yet. Import video or audio to start.</li>
        )}
      </ul>
      {selected?.has_waveform && <Waveform api={api} id={selected.id} />}
      <CacheSection
        api={api}
        refreshKey={media.cacheKey}
        busy={progress.running}
        onCleared={media.refresh}
      />
    </section>
  );
};
