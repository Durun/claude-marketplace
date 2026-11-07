import { describe, expect, it, vi } from "vitest";
import type { Op } from "@mm/sdk";
import { createIpcStoryApi } from "./api.ts";

describe("createIpcStoryApi", () => {
  it("コマンドの名前と引数をアプリの形で渡す", async () => {
    const call = vi.fn().mockResolvedValue(null);
    const api = createIpcStoryApi(call);
    await api.overview();
    await api.transcript("a1");
    await api.range("a1", "w1", "w2");
    await api.search("編集", 10);
    await api.timelineTime("a1", 1.5);
    await api.cutTime("k1");
    await api.seek(3);
    expect(call.mock.calls).toEqual([
      ["story_overview"],
      ["story_transcript", { asset: "a1" }],
      ["story_range", { asset: "a1", first: "w1", last: "w2" }],
      ["story_search", { query: "編集", limit: 10 }],
      ["story_timeline_time", { asset: "a1", at: 1.5 }],
      ["story_cut_time", { cut: "k1" }],
      ["transport", { action: "seek", value: 3 }],
    ]);
  });

  it("操作は apply_op で送り、拒否された理由を返す", async () => {
    const call = vi.fn().mockRejectedValue("既にその状態");
    const api = createIpcStoryApi(call);
    const op: Op = { op: "deleteWords", asset: "a1", words: ["w1"] };
    expect(await api.apply(op)).toEqual({ ok: false, error: "既にその状態" });
    expect(call).toHaveBeenCalledWith("apply_op", { op });
  });
});
