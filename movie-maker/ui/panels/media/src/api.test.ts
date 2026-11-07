import { describe, expect, it, vi } from "vitest";
import type { MediaListen } from "@mm/sdk";
import { createIpcMediaApi } from "./api.ts";

type Handler = (e: { payload: unknown }) => void;

const fakeListen = () => {
  const handlers = new Map<string, Handler>();
  const listen: MediaListen = async (name, h) => {
    handlers.set(name, h);
    return () => {
      handlers.delete(name);
    };
  };
  return { handlers, listen };
};

describe("createIpcMediaApi", () => {
  it("コマンドの名前と引数をアプリの形で渡す", async () => {
    const call = vi.fn().mockResolvedValue([]);
    const api = createIpcMediaApi(call, vi.fn());
    await api.list();
    await api.thumbnail("a1");
    await api.waveform("a1", 50);
    await api.importFiles(["C:/clip.mp4"]);
    await api.clearCache(["preview"]);
    expect(call.mock.calls).toEqual([
      ["media_list"],
      ["media_thumbnail", { id: "a1" }],
      ["media_waveform", { id: "a1", buckets: 50 }],
      ["media_import", { paths: ["C:/clip.mp4"] }],
      ["media_cache_clear", { kinds: ["preview"] }],
    ]);
  });

  it("取り込みの進み具合のイベントを購読し、外せる", async () => {
    const { handlers, listen } = fakeListen();
    const seen = vi.fn();
    const off = await createIpcMediaApi(vi.fn(), listen).onProgress(seen);
    const finished = { type: "finished", added: 1, skipped: 0, failed: 0 };
    handlers.get("mm:ingest-progress")?.({ payload: finished });
    expect(seen).toHaveBeenCalledWith(finished);
    off();
    expect(handlers.size).toBe(0);
  });

  it("ドロップのイベントをパスつきで届ける", async () => {
    const { handlers, listen } = fakeListen();
    const seen = vi.fn();
    await createIpcMediaApi(vi.fn(), listen).onDrop(seen);
    handlers.get("tauri://drag-enter")?.({ payload: {} });
    handlers.get("tauri://drag-drop")?.({ payload: { paths: ["C:/a.mp4"] } });
    expect(seen.mock.calls).toEqual([[{ type: "enter" }], [{ type: "drop", paths: ["C:/a.mp4"] }]]);
  });
});
