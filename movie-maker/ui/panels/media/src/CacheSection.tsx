import { useEffect, useState } from "preact/hooks";
import type { CacheFolder, CacheInfo, MediaApi } from "./api.ts";
import { formatBytes } from "./format.ts";

export interface CacheSectionProps {
  api: MediaApi;
  /** 値が変わるたびに大きさを読み直す（取り込みの後など）。 */
  refreshKey: unknown;
  /** 取り込み中か。取り込みが作っている途中の出力を消さないよう、その間は掃除を出さない。 */
  busy?: boolean;
  /** 掃除した後に呼ぶ。素材の状態を読み直すのに使う。 */
  onCleared: () => void;
}

interface ConfirmProps {
  folders: CacheFolder[];
  onDelete: () => void;
  onCancel: () => void;
}

/** 何をどれだけ消すかを見せて、消すかどうかを確かめる。 */
const Confirm = ({ folders, onDelete, onCancel }: ConfirmProps) => (
  <div class="mm-media-confirm" role="alertdialog" aria-label="Clean up the cache">
    <ul class="mm-media-cache-list">
      {folders.map((f) => (
        <li key={f.name}>
          {f.name} · {f.files} files · {formatBytes(f.bytes)}
        </li>
      ))}
    </ul>
    <p class="mm-media-note">Import the clips again to rebuild them.</p>
    <div class="mm-media-cache-row">
      <button type="button" class="mm-media-button mm-media-primary" onClick={onDelete}>
        Delete {formatBytes(folders.reduce((sum, f) => sum + f.bytes, 0))}
      </button>
      <button type="button" class="mm-media-button" onClick={onCancel}>
        Cancel
      </button>
    </div>
  </div>
);

/** `cache/` の大きさと、掃除のボタン。 */
export const CacheSection = ({ api, refreshKey, busy = false, onCleared }: CacheSectionProps) => {
  const [info, setInfo] = useState<CacheInfo | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    api
      .cacheInfo()
      .then(setInfo)
      .catch((e: unknown) => setError(String(e)));
  };
  useEffect(load, [api, refreshKey]);

  const clearable = info?.folders.filter((f) => f.clearable) ?? [];

  const clear = () => {
    api
      .clearCache(clearable.map((f) => f.name))
      .then(() => {
        setConfirming(false);
        load();
        onCleared();
      })
      .catch((e: unknown) => setError(String(e)));
  };

  return (
    <section class="mm-media-cache" aria-label="Cache">
      <div class="mm-media-cache-row">
        <span>
          Cache <strong>{info ? formatBytes(info.total_bytes) : "…"}</strong>
        </span>
        <button
          type="button"
          class="mm-media-button"
          disabled={busy || clearable.every((f) => f.bytes === 0)}
          onClick={() => setConfirming(true)}
        >
          Clean up…
        </button>
      </div>
      {confirming && !busy && (
        <Confirm folders={clearable} onDelete={clear} onCancel={() => setConfirming(false)} />
      )}
      {error && <p class="mm-media-error">{error}</p>}
    </section>
  );
};
