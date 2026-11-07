import type { ExportItem, ExportStatus, ExportVerification } from "./api.ts";
import { formatBytes, formatDuration, PERCENT, percent } from "./format.ts";

const STATE_LABEL: Record<ExportItem["state"], string> = {
  queued: "Waiting",
  running: "Exporting",
  done: "Done",
  failed: "Failed",
  cancelled: "Cancelled",
};

const Verified = ({ v }: { v: ExportVerification | null }) =>
  v ? (
    <div class="mm-deliver-meta" data-verified="true">
      Verified: {v.video_codec} {v.width}×{v.height}, {v.video_frames} frames, {formatDuration(v.video_seconds)}
      {v.audio_seconds !== null && `, audio ${formatDuration(v.audio_seconds)}`}
    </div>
  ) : (
    <div class="mm-deliver-meta" data-verified="false">
      Not verified
    </div>
  );

const Progress = ({ item }: { item: ExportItem }) => {
  const pct = percent(item.frames_done, item.frames_total);
  return (
    <>
      <div class="mm-deliver-bar" role="progressbar" aria-label={`Export ${item.target}`} aria-valuemin={0} aria-valuemax={PERCENT} aria-valuenow={pct}>
        <div class="mm-deliver-bar-fill" style={{ width: `${pct}%` }} />
      </div>
      <div class="mm-deliver-meta">
        {item.frames_done} / {item.frames_total || "?"} frames · {item.fps.toFixed(1)} fps
        {item.eta !== null && ` · ${formatDuration(item.eta)} left`}
      </div>
    </>
  );
};

const Result = ({ item, onReveal }: { item: ExportItem; onReveal: (path: string) => void }) => (
  <>
    <div class="mm-deliver-meta">
      {item.frames_total} frames in {formatDuration(item.elapsed)} · {formatBytes(item.bytes_written)}
    </div>
    <Verified v={item.verification} />
    <div class="mm-deliver-row-actions">
      <button type="button" class="mm-deliver-button" onClick={() => onReveal(item.output_path)}>
        Open folder
      </button>
    </div>
  </>
);

const Item = ({ item, onReveal }: { item: ExportItem; onReveal: (path: string) => void }) => (
  <li class="mm-deliver-item" data-state={item.state}>
    <div class="mm-deliver-row-head">
      <strong class="mm-deliver-name">{item.target}</strong>
      <span class="mm-deliver-state">{STATE_LABEL[item.state]}</span>
    </div>
    {item.state === "running" && <Progress item={item} />}
    {item.state === "done" && <Result item={item} onReveal={onReveal} />}
    {item.state === "failed" && (
      <p class="mm-deliver-error" role="alert">
        {item.error}
      </p>
    )}
    <div class="mm-deliver-meta mm-deliver-path" title={item.output_path}>
      {item.output_path}
    </div>
  </li>
);

export interface ExportQueueProps {
  status: ExportStatus;
  onCancel: () => void;
  onReveal: (path: string) => void;
}

/** 書き出しのキュー。1 本ずつの進み具合、取り消し、終わったあとの検証の結果と「フォルダーを開く」。 */
export const ExportQueue = ({ status, onCancel, onReveal }: ExportQueueProps) => {
  if (status.items.length === 0) return null;
  const finished = status.items.filter((i) => i.state === "done").length;
  return (
    <section class="mm-deliver-queue" aria-label="Export queue">
      <div class="mm-deliver-row-head">
        <h3 class="mm-deliver-heading">
          {status.running ? "Exporting" : "Export finished"} ({finished}/{status.items.length})
        </h3>
        {status.running && (
          <button type="button" class="mm-deliver-button" onClick={onCancel}>
            Cancel
          </button>
        )}
      </div>
      <ul class="mm-deliver-items">
        {status.items.map((item) => (
          <Item key={item.target} item={item} onReveal={onReveal} />
        ))}
      </ul>
    </section>
  );
};
