// Media パネルをモックの API で動かすページ。スクリーンショット用で、アプリには載せない。
// ?state=idle|importing|failed|empty|cache で状態を選ぶ。
import { render } from "preact";
import type { CacheInfo, ImportEvent, MediaApi, MediaItem } from "../src/api.ts";
import { followTheme } from "../../../shell/src/followTheme.ts";
import { MediaPanel } from "../src/index.ts";
import "../../../shell/src/shell.css";

const HASH = "0123456789abcdef";
const DEMO_WIDTH = 320;
const DEMO_HEIGHT = 180;
const PEAKS = 120;
const HUE_STEP = 70;
const BYTES_MB = 1024 * 1024;

const clip = (n: number, over: Partial<MediaItem>): MediaItem => ({
  id: `a_${n}`,
  name: `clip-${n}.mp4`,
  path: `C:/Videos/clip-${n}.mp4`,
  content_hash: `${HASH}${n}`,
  kind: "video",
  duration: 2 + n * 37,
  fps: 30,
  width: 1920,
  height: 1080,
  has_thumbnail: true,
  has_waveform: true,
  status: "ready",
  ...over,
});

const ITEMS: MediaItem[] = [
  clip(1, { name: "interview-take1.mp4", duration: 312 }),
  clip(2, { name: "broll-harbor.mov", fps: 59.94, width: 3840, height: 2160, duration: 48 }),
  clip(3, { name: "narration.wav", kind: "audio", width: null, height: null, fps: null, has_thumbnail: false, duration: 95 }),
  clip(4, { name: "drone-old.mp4", has_thumbnail: false, status: "partial" }),
  clip(5, { name: "moved-away.mp4", status: "missing" }),
];

const CACHE: CacheInfo = {
  total_bytes: 412 * BYTES_MB,
  folders: [
    { name: "preview", bytes: 380 * BYTES_MB, files: 4, clearable: true },
    { name: "audio", bytes: 2 * BYTES_MB, files: 3, clearable: true },
    { name: "thumbs", bytes: 1 * BYTES_MB, files: 4, clearable: true },
    { name: "ingest", bytes: 29 * BYTES_MB, files: 6, clearable: false },
  ],
};

/** 色を変えた単色のサムネイルを JPEG にする。 */
const thumbnail = async (id: string): Promise<ArrayBuffer> => {
  const canvas = document.createElement("canvas");
  canvas.width = DEMO_WIDTH;
  canvas.height = DEMO_HEIGHT;
  const g = canvas.getContext("2d");
  if (g) {
    const hue = (Number(id.slice(-1)) * HUE_STEP) % 360;
    g.fillStyle = `hsl(${hue} 45% 45%)`;
    g.fillRect(0, 0, DEMO_WIDTH, DEMO_HEIGHT);
    g.fillStyle = `hsl(${hue} 45% 70%)`;
    g.fillRect(DEMO_WIDTH / 4, DEMO_HEIGHT / 4, DEMO_WIDTH / 2, DEMO_HEIGHT / 2);
  }
  const blob = await new Promise<Blob | null>((ok) => canvas.toBlob(ok, "image/jpeg"));
  return blob ? blob.arrayBuffer() : new ArrayBuffer(0);
};

const waveform = async (id: string, buckets: number) => ({
  duration: 95,
  peaks: Array.from({ length: Math.min(buckets, PEAKS) }, (_, i) => {
    const swell = Math.abs(Math.sin(i / 7 + Number(id.slice(-1))));
    return Math.min(1, 0.08 + 0.85 * swell * Math.abs(Math.sin(i / 3)));
  }),
});

const scene = new URLSearchParams(location.search).get("state") ?? "idle";
const listeners: ((e: ImportEvent) => void)[] = [];
const push = (e: ImportEvent) => listeners.forEach((l) => l(e));

const api: MediaApi = {
  list: async () => (scene === "empty" ? [] : ITEMS),
  thumbnail,
  waveform,
  pickFiles: async () => [],
  importFiles: async () => {},
  cacheInfo: async () => CACHE,
  clearCache: async () => 0,
  onProgress: async (h) => {
    listeners.push(h);
    return () => {};
  },
  onDrop: async () => () => {},
};

followTheme(document.documentElement, location.search, matchMedia.bind(window));
const root = document.getElementById("root");
if (root) {
  const host = document.createElement("aside");
  host.className = "mm-shell-left";
  root.append(host);
  render(<MediaPanel api={api} />, host);
}

/** 状態ごとに、パネルが出来たあとでイベントを流す。 */
const STAGE_DELAY_MS = 300;
window.setTimeout(() => {
  if (scene === "importing") {
    push({ type: "started", index: 1, total: 3, path: "C:/Videos/holiday-footage.mp4" });
  }
  if (scene === "failed") {
    push({ type: "started", index: 0, total: 1, path: "C:/Videos/notes.txt" });
    push({ type: "failed", index: 0, path: "C:/Videos/notes.txt", error: "取り込めない素材: 映像も音も無い" });
    push({ type: "finished", added: 0, skipped: 0, failed: 1 });
  }
  if (scene === "cache") {
    document.querySelector<HTMLButtonElement>(".mm-media-cache .mm-media-button")?.click();
  }
}, STAGE_DELAY_MS);
