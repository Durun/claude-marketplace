import { describe, expect, it, vi } from "vitest";
import type { MediaListen } from "@mm/sdk";
import { createIpcReviewApi, PROPOSALS_CHANGED_EVENT } from "./api.ts";

type Handler = (e: { payload: unknown }) => void;

describe("createIpcReviewApi", () => {
  it("コマンドの名前と引数をアプリの形で渡す", async () => {
    const call = vi.fn().mockResolvedValue(null);
    const api = createIpcReviewApi(call, vi.fn());
    await api.list();
    await api.approve(3, [0, 2]);
    await api.approve(4, null);
    await api.reject(5);
    await api.reopen(5);
    await api.undo(3);
    await api.preview(3, [0]);
    await api.preview(null, null);
    expect(call.mock.calls).toEqual([
      ["review_list", undefined],
      ["review_approve", { id: 3, ops: [0, 2] }],
      ["review_approve", { id: 4, ops: null }],
      ["review_reject", { id: 5 }],
      ["review_reopen", { id: 5 }],
      ["review_undo", { id: 3 }],
      ["review_preview", { id: 3, ops: [0] }],
      ["review_preview", { id: null, ops: null }],
    ]);
  });

  it("アプリの失敗（文字列）を理由つきの Error にする", async () => {
    const call = vi.fn().mockRejectedValue("conflict: k1");
    const api = createIpcReviewApi(call, vi.fn());
    await expect(api.approve(1, null)).rejects.toThrow("conflict: k1");
  });

  it("イベントの中身をそのまま渡し、購読を外せる", async () => {
    const handlers = new Map<string, Handler>();
    const off = vi.fn();
    const listen: MediaListen = async (name, h) => {
      handlers.set(name, h as Handler);
      return off;
    };
    const api = createIpcReviewApi(vi.fn(), listen);
    const seen = vi.fn();
    const stop = await api.onChanged(seen);
    handlers.get(PROPOSALS_CHANGED_EVENT)?.({ payload: { id: 2, state: "pending" } });
    expect(seen).toHaveBeenCalledWith({ id: 2, state: "pending" });
    stop();
    expect(off).toHaveBeenCalled();
  });
});
