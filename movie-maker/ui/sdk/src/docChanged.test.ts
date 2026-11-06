import { describe, expect, it, vi } from "vitest";
import { DOC_CHANGED_EVENT, onDocChanged } from "./docChanged.ts";

describe("onDocChanged", () => {
  it("mm:doc-changed を購読し、中身を渡す", async () => {
    const off = vi.fn();
    const listen = vi.fn().mockResolvedValue(off);
    const handler = vi.fn();
    const stop = await onDocChanged(handler, listen);
    expect(listen.mock.calls[0]?.[0]).toBe(DOC_CHANGED_EVENT);
    const payload = { version: 3, txn_id: "x", kind: "op", name: "n", actor: "cli", touched: ["a"] };
    listen.mock.calls[0]?.[1]({ payload });
    expect(handler).toHaveBeenCalledWith(payload);
    expect(stop).toBe(off);
  });
});
