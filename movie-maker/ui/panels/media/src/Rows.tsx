import type { MediaApi, MediaItem } from "./api.ts";
import { baseName, describeItem, STATUS_LABEL } from "./format.ts";
import type { ImportProgress } from "./progress.ts";
import { Thumb } from "./Thumb.tsx";

export interface RowProps {
  api: MediaApi;
  item: MediaItem;
  selected: boolean;
  onSelect: () => void;
}

/** 素材 1 つの行。サムネイル、名前、長さ・解像度・フレームレート、取り込みの状態。 */
export const Row = ({ api, item, selected, onSelect }: RowProps) => (
  <li>
    <button
      type="button"
      class="mm-media-row"
      aria-pressed={selected}
      data-status={item.status}
      onClick={onSelect}
    >
      <Thumb api={api} item={item} />
      <span class="mm-media-text">
        <span class="mm-media-name" title={item.path}>
          {item.name}
        </span>
        <span class="mm-media-meta">{describeItem(item)}</span>
        <span class="mm-media-status">{STATUS_LABEL[item.status]}</span>
      </span>
    </button>
  </li>
);

/** いま取り込んでいる素材の行。取り込みが終わるまで一覧の先頭に出す。 */
export const PendingRow = ({ progress }: { progress: ImportProgress }) => (
  <li>
    <div class="mm-media-row mm-media-pending" role="status">
      <span class="mm-media-thumb mm-media-thumb-empty" aria-hidden="true" />
      <span class="mm-media-text">
        <span class="mm-media-name">{baseName(progress.current ?? "")}</span>
        <span class="mm-media-meta">
          Importing {progress.index + 1} of {progress.total}…
        </span>
        <span class="mm-media-bar" aria-hidden="true" />
      </span>
    </div>
  </li>
);

export interface FailuresProps {
  failures: ImportProgress["failures"];
  onDismiss: () => void;
}

/** 取り込めなかった素材と、その理由。 */
export const Failures = ({ failures, onDismiss }: FailuresProps) => (
  <div class="mm-media-failures" role="alert">
    <ul>
      {failures.map((f) => (
        <li key={f.path}>
          <strong>{baseName(f.path)}</strong>: {f.error}
        </li>
      ))}
    </ul>
    <button type="button" class="mm-media-button" onClick={onDismiss}>
      Dismiss
    </button>
  </div>
);
