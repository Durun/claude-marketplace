// @vitest-environment jsdom
import { render } from "preact";
import { act } from "preact/test-utils";
import type { DropEvent, ImportEvent, MediaApi, MediaItem } from "./api.ts";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MediaPanel } from "./MediaPanel.tsx";

const host = document.createElement("div");
document.body.append(host);
afterEach(() => render(null, host));

const item = (over: Partial<MediaItem> = {}): MediaItem => ({
  id: "a_1",
  name: "clip.mp4",
  path: "C:/Videos/clip.mp4",
  content_hash: "h1",
  kind: "video",
  duration: 75,
  fps: 30,
  width: 1920,
  height: 1080,
  has_thumbnail: false,
  has_waveform: true,
  status: "ready",
  ...over,
});

interface Fake {
  api: MediaApi;
  progress: (e: ImportEvent) => void;
  drop: (e: DropEvent) => void;
}

const settle = () => act(() => new Promise<void>((ok) => setTimeout(ok, 0)));

const mount = async (items: MediaItem[], over: Partial<MediaApi> = {}): Promise<Fake> => {
  const handlers: { progress: (e: ImportEvent) => void; drop: (e: DropEvent) => void } = {
    progress: () => {},
    drop: () => {},
  };
  const api: MediaApi = {
    list: vi.fn().mockResolvedValue(items),
    thumbnail: vi.fn().mockResolvedValue(new ArrayBuffer(0)),
    waveform: vi.fn().mockResolvedValue({ duration: 75, peaks: [0.2, 0.8, 0.4] }),
    pickFiles: vi.fn().mockResolvedValue([]),
    importFiles: vi.fn().mockResolvedValue(undefined),
    cacheInfo: vi.fn().mockResolvedValue({
      total_bytes: 3 * 1024 * 1024,
      folders: [
        { name: "preview", bytes: 3 * 1024 * 1024, files: 2, clearable: true },
        { name: "ingest", bytes: 10, files: 1, clearable: false },
      ],
    }),
    clearCache: vi.fn().mockResolvedValue(0),
    onProgress: vi.fn(async (h) => {
      handlers.progress = h;
      return () => {};
    }),
    onDrop: vi.fn(async (h) => {
      handlers.drop = h;
      return () => {};
    }),
    ...over,
  };
  act(() => render(<MediaPanel api={api} />, host));
  await settle();
  return { api, progress: (e) => act(() => handlers.progress(e)), drop: (e) => act(() => handlers.drop(e)) };
};

const click = (el: Element | null) =>
  act(() => {
    (el as HTMLElement).click();
  });

describe("MediaPanel", () => {
  it("素材の名前、長さ、解像度、フレームレート、状態を行に出す", async () => {
    await mount([item(), item({ id: "a_2", name: "gone.mp4", status: "missing" })]);
    const rows = host.querySelectorAll(".mm-media-row");
    expect(rows[0]?.textContent).toContain("clip.mp4");
    expect(rows[0]?.textContent).toContain("1:15 · 1920×1080 · 30 fps");
    expect(rows[0]?.textContent).toContain("Ready");
    expect(rows[1]?.textContent).toContain("File missing");
  });

  it("素材が無いときは取り込みへ促す", async () => {
    await mount([]);
    expect(host.textContent).toContain("No clips yet");
  });

  it("素材を選ぶと波形を出す", async () => {
    const { api } = await mount([item()]);
    click(host.querySelector(".mm-media-row"));
    await settle();
    expect(api.waveform).toHaveBeenCalledWith("a_1", 120);
    expect(host.querySelector(".mm-media-wave-svg path")).not.toBeNull();
  });

  it("Import を押すと選んだファイルを取り込む", async () => {
    const { api } = await mount([], { pickFiles: vi.fn().mockResolvedValue(["C:/a.mp4"]) });
    click(host.querySelector(".mm-media-primary"));
    await settle();
    expect(api.importFiles).toHaveBeenCalledWith(["C:/a.mp4"]);
  });

  it("ファイルのドロップで取り込み、ドラッグ中は枠で示す", async () => {
    const { api, drop } = await mount([]);
    drop({ type: "enter" });
    expect(host.querySelector(".mm-media")?.getAttribute("data-dragging")).toBe("true");
    drop({ type: "drop", paths: ["C:/a.mp4", "C:/b.wav"] });
    expect(host.querySelector(".mm-media")?.getAttribute("data-dragging")).toBe("false");
    expect(api.importFiles).toHaveBeenCalledWith(["C:/a.mp4", "C:/b.wav"]);
  });

});

describe("MediaPanel の取り込みとキャッシュ", () => {
  it("取り込み中の素材を先頭の行に出し、終わったら一覧を読み直す", async () => {
    const { api, progress } = await mount([]);
    progress({ type: "started", index: 1, total: 3, path: "C:\\Videos\\new.mp4" });
    const pending = host.querySelector(".mm-media-pending");
    expect(pending?.textContent).toContain("new.mp4");
    expect(pending?.textContent).toContain("Importing 2 of 3");
    expect(host.querySelector<HTMLButtonElement>(".mm-media-primary")?.disabled).toBe(true);
    progress({ type: "finished", added: 1, skipped: 0, failed: 0 });
    await settle();
    expect(host.querySelector(".mm-media-pending")).toBeNull();
    expect(vi.mocked(api.list).mock.calls.length).toBeGreaterThan(1);
  });

  it("失敗した素材と理由を出し、閉じられる", async () => {
    const { progress } = await mount([]);
    progress({ type: "started", index: 0, total: 1, path: "C:/notes.txt" });
    progress({ type: "failed", index: 0, path: "C:/notes.txt", error: "取り込めない" });
    progress({ type: "finished", added: 0, skipped: 0, failed: 1 });
    expect(host.querySelector(".mm-media-failures")?.textContent).toContain("notes.txt");
    expect(host.querySelector(".mm-media-failures")?.textContent).toContain("取り込めない");
    click(host.querySelector(".mm-media-failures button"));
    expect(host.querySelector(".mm-media-failures")).toBeNull();
  });

  it("キャッシュの大きさを出し、掃除は確かめてから掃除できるものだけを消す", async () => {
    const { api } = await mount([]);
    expect(host.querySelector(".mm-media-cache")?.textContent).toContain("3.0 MB");
    click(host.querySelector(".mm-media-cache .mm-media-button"));
    expect(host.querySelector(".mm-media-confirm")?.textContent).toContain("preview");
    expect(host.querySelector(".mm-media-confirm")?.textContent).not.toContain("ingest");
    expect(api.clearCache).not.toHaveBeenCalled();
    click(host.querySelector(".mm-media-confirm .mm-media-primary"));
    await settle();
    expect(api.clearCache).toHaveBeenCalledWith(["preview"]);
    expect(host.querySelector(".mm-media-confirm")).toBeNull();
  });

  it("取り込み中は掃除を出さない", async () => {
    const { progress } = await mount([]);
    const cleanUp = () => host.querySelector<HTMLButtonElement>(".mm-media-cache .mm-media-button");
    expect(cleanUp()?.disabled).toBe(false);
    progress({ type: "started", index: 0, total: 1, path: "C:/a.mp4" });
    expect(cleanUp()?.disabled).toBe(true);
    progress({ type: "finished", added: 1, skipped: 0, failed: 0 });
    await settle();
    expect(cleanUp()?.disabled).toBe(false);
  });
});
