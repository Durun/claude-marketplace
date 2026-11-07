import { describe, expect, it, vi } from "vitest";
import type { MediaListen } from "@mm/sdk";
import { createIpcDeliverApi, EXPORT_PROGRESS_EVENT } from "./api.ts";

type Handler = (e: { payload: unknown }) => void;

describe("createIpcDeliverApi", () => {
  it("コマンドの名前と引数をアプリの形で渡す", async () => {
    const call = vi.fn().mockResolvedValue(undefined);
    const api = createIpcDeliverApi(call, vi.fn());
    const op = { op: "removeRenderTarget" as const, target: "yt" };
    await api.overview();
    await api.status();
    await api.applyOps([op]);
    await api.start({ targets: ["yt", "shorts"], sequence: "main", verify: true });
    await api.cancel();
    await api.reveal("C:/out/yt.mp4");
    await api.pickOutput("C:/out/yt.mp4", "prores_ks");
    expect(call.mock.calls).toEqual([
      ["export_overview", undefined],
      ["export_status", undefined],
      ["apply_ops", { ops: [op] }],
      ["export_start", { request: { targets: ["yt", "shorts"], sequence: "main", verify: true } }],
      ["export_cancel", undefined],
      ["export_reveal", { path: "C:/out/yt.mp4" }],
      ["export_pick_output", { current: "C:/out/yt.mp4", codec: "prores_ks" }],
    ]);
  });

  it("アプリの失敗（文字列）を理由つきの Error にする", async () => {
    const call = vi.fn().mockRejectedValue("書き出し中");
    const api = createIpcDeliverApi(call, vi.fn());
    await expect(api.start({ targets: ["a"], verify: false })).rejects.toThrow("書き出し中");
  });

  it("進み具合のイベントの中身（キューの状態）をそのまま渡し、購読を外せる", async () => {
    const handlers = new Map<string, Handler>();
    const listen: MediaListen = async (name, h) => {
      handlers.set(name, h);
      return () => {
        handlers.delete(name);
      };
    };
    const seen = vi.fn();
    const off = await createIpcDeliverApi(vi.fn(), listen).onProgress(seen);
    const status = { running: true, items: [] };
    handlers.get(EXPORT_PROGRESS_EVENT)?.({ payload: status });
    expect(seen).toHaveBeenCalledWith(status);
    off();
    expect(handlers.size).toBe(0);
  });
});
