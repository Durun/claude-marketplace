import type { MediaItem, MediaStatus } from "./api.ts";

const SECONDS_PER_MINUTE = 60;
const SECONDS_PER_HOUR = 3600;
const BYTES_PER_UNIT = 1024;
const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;
const PAD = 2;

/** 秒を `m:ss`（1 時間以上は `h:mm:ss`）にする。 */
export const formatDuration = (seconds: number): string => {
  const total = Math.max(0, Math.round(seconds));
  const h = Math.floor(total / SECONDS_PER_HOUR);
  const m = Math.floor((total % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE);
  const s = String(total % SECONDS_PER_MINUTE).padStart(PAD, "0");
  return h > 0 ? `${h}:${String(m).padStart(PAD, "0")}:${s}` : `${m}:${s}`;
};

/** バイト数を `12.3 MB` のように読みやすくする。 */
export const formatBytes = (bytes: number): string => {
  let value = Math.max(0, bytes);
  let unit = 0;
  while (value >= BYTES_PER_UNIT && unit < UNITS.length - 1) {
    value /= BYTES_PER_UNIT;
    unit += 1;
  }
  return unit === 0 ? `${value} B` : `${value.toFixed(1)} ${UNITS[unit]}`;
};

const trimFps = (fps: number): string => String(Number(fps.toFixed(PAD)));

/** 一覧の行の 2 行目に出す、長さ、解像度、フレームレート。分からないものは出さない。 */
export const describeItem = (item: MediaItem): string =>
  [
    formatDuration(item.duration),
    item.width && item.height ? `${item.width}×${item.height}` : null,
    item.fps ? `${trimFps(item.fps)} fps` : null,
  ]
    .filter((part): part is string => part !== null)
    .join(" · ");

/** 取り込みの状態の表示名。 */
export const STATUS_LABEL: Record<MediaStatus, string> = {
  ready: "Ready",
  partial: "Incomplete",
  missing: "File missing",
};

/** パスの末尾のファイル名（`\` と `/` のどちらでも）。 */
export const baseName = (path: string): string => path.split(/[\\/]/).pop() ?? path;
