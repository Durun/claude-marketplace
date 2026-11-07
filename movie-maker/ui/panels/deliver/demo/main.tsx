// Deliver パネルをモックの API で動かすページ。スクリーンショット用で、アプリには載せない。
// 書き出しは `window.__deliverStep(frames)` を呼んだ分だけ進む（撮る状態を決められるように、時間では進めない）。
import { render } from "preact";
import type { Op, RenderTarget } from "@mm/sdk";
import type { DeliverApi, DeliverOverview, ExportItem, ExportRequest, ExportStatus } from "../src/api.ts";
import { followTheme } from "../../../shell/src/followTheme.ts";
import { DeliverPanel } from "../src/index.ts";
import "../../../shell/src/shell.css";

const FRAMES = 1260;
const FPS = 30;
const MEASURED_FPS = 142.6;

const targets: RenderTarget[] = [
  { id: "youtube-1080p", composition: "main", width: 1920, height: 1080, fps: { num: 30, den: 1 }, codec: "h264", output_path: "D:/videos/tutorial/exports/youtube-1080p.mp4" },
  { id: "youtube-shorts", composition: "main", width: 1080, height: 1920, fps: { num: 30, den: 1 }, codec: "h264", output_path: "D:/videos/tutorial/exports/youtube-shorts.mp4", loudness_lufs: -14 },
  { id: "prores-master", composition: "main", width: 1920, height: 1080, fps: { num: 30, den: 1 }, codec: "prores_ks", output_path: "D:/videos/tutorial/exports/master.mov" },
];

const overview = (): DeliverOverview => ({
  targets: targets.map((t) => ({ ...t })),
  compositions: [
    { id: "main", duration: FRAMES / FPS, width: 1920, height: 1080 },
    { id: "short", duration: 38, width: 1080, height: 1920 },
  ],
  sequences: [{ id: "main-cut", duration: 38 }],
  open_composition: "main",
  can_verify: true,
});

let status: ExportStatus = { running: false, items: [] };
const listeners = new Set<(s: ExportStatus) => void>();
const publish = (next: ExportStatus): void => {
  status = next;
  listeners.forEach((l) => l(status));
};

const queued = (id: string): ExportItem => {
  const t = targets.find((x) => x.id === id) as RenderTarget;
  return { target: id, state: "queued", frames_done: 0, frames_total: 0, fps: 0, elapsed: 0, eta: null, output_path: t.output_path, bytes_written: 0, verification: null, error: null };
};

/** 先頭の未完了の項目を `frames` フレーム進める。 */
const step = (frames: number): void => {
  const items = status.items.map((i) => ({ ...i }));
  const item = items.find((i) => i.state === "queued" || i.state === "running");
  if (!item) return;
  item.state = "running";
  item.frames_total = FRAMES;
  item.frames_done = Math.min(FRAMES, item.frames_done + frames);
  item.fps = MEASURED_FPS;
  item.elapsed = item.frames_done / MEASURED_FPS;
  item.eta = (FRAMES - item.frames_done) / MEASURED_FPS;
  if (item.frames_done >= FRAMES) {
    item.state = "done";
    item.bytes_written = item.target.includes("prores") ? 912_000_000 : 48_300_000;
    item.verification = { video_codec: item.target.includes("prores") ? "prores" : "h264", width: 1920, height: 1080, video_frames: FRAMES, video_seconds: FRAMES / FPS, audio_seconds: FRAMES / FPS };
  }
  publish({ running: items.some((i) => i.state === "queued" || i.state === "running"), items });
};

(window as unknown as { __deliverStep: (n: number) => void }).__deliverStep = step;

const apply = (op: Op): void => {
  if (op.op === "addRenderTarget") targets.splice(op.index ?? targets.length, 0, op.target);
  if (op.op === "removeRenderTarget") targets.splice(targets.findIndex((t) => t.id === op.target), 1);
};

const api: DeliverApi = {
  overview: async () => overview(),
  status: async () => status,
  applyOps: async (ops) => ops.forEach(apply),
  start: async (request: ExportRequest) => publish({ running: true, items: request.targets.map(queued) }),
  cancel: async () => publish({ running: false, items: status.items.map((i) => (i.state === "done" ? i : { ...i, state: "cancelled" as const })) }),
  reveal: async (path) => console.debug("reveal", path),
  pickOutput: async (current) => current ?? null,
  onProgress: async (handler) => {
    listeners.add(handler);
    return () => void listeners.delete(handler);
  },
};

followTheme(document.documentElement, location.search, matchMedia.bind(window));
const root = document.getElementById("root");
if (root) {
  root.style.boxSizing = "border-box";
  root.style.flexDirection = "column";
  root.style.padding = "10px";
  root.style.width = "260px";
  root.style.height = "100vh";
  render(<DeliverPanel api={api} />, root);
}
