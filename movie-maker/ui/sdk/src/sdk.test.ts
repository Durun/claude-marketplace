import { describe, expect, it, vi } from "vitest";
import { invoke } from "./index.ts";

describe("invoke", () => {
  it("まだつながっていないので、操作の名前つきの失敗を返す", async () => {
    const debug = vi.spyOn(console, "debug").mockImplementation(() => {});
    const result = await invoke("clip.split");
    expect(result.ok).toBe(false);
    expect(debug).toHaveBeenCalled();
  });
});
