// @vitest-environment jsdom
import { render } from "preact";
import { act } from "preact/test-utils";
import type { Proposal, ReviewApi } from "./api.ts";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ReviewPanel } from "./ReviewPanel.tsx";

const host = document.createElement("div");
document.body.append(host);
afterEach(() => render(null, host));

const NOW = 1_000_000_000;

const proposal = (id: number, over: Partial<Proposal> = {}): Proposal => ({
  id,
  name: `Proposal ${id}`,
  actor: "mcp",
  actor_name: "claude",
  at_ms: NOW - 180_000,
  responds_to: null,
  base_version: 1,
  state: "pending",
  decided_by: null,
  decided_at_ms: null,
  txn_id: null,
  approved_ops: null,
  applied: false,
  undoable: false,
  conflict: null,
  ops: [
    { index: 0, op: { op: "extract", cut: "k1" }, touches: ["k1"] },
    { index: 1, op: { op: "lift", cut: "k2" }, touches: ["k2"] },
    { index: 2, op: { op: "extract", cut: "k3" }, touches: ["k3"] },
  ],
  scope: { sequences: [{ id: "s1", cuts: ["k1", "k2"], start: 1.5, end: 4 }], layers: [], other: [] },
  ...over,
});

const fakeApi = (list: Proposal[], over: Partial<ReviewApi> = {}): ReviewApi => ({
  list: vi.fn().mockResolvedValue(list),
  approve: vi.fn().mockResolvedValue({ proposal: list[0], change: null }),
  reject: vi.fn().mockResolvedValue(list[0]),
  reopen: vi.fn().mockResolvedValue(list[0]),
  undo: vi.fn().mockResolvedValue(list[0]),
  preview: vi.fn().mockResolvedValue(undefined),
  onChanged: vi.fn().mockResolvedValue(() => undefined),
  ...over,
});

const settle = () => act(() => new Promise<void>((ok) => setTimeout(ok, 0)));

const mount = async (api: ReviewApi) => {
  act(() => render(<ReviewPanel api={api} nowMs={NOW} />, host));
  await settle();
};

const button = (name: string): HTMLButtonElement | undefined =>
  [...host.querySelectorAll("button")].find((b) => b.textContent === name);

const click = async (el: Element | null | undefined) => {
  act(() => (el as HTMLElement).click());
  await settle();
};

describe("ReviewPanel の一覧", () => {
  it("提案が無いときは案内を出す", async () => {
    await mount(fakeApi([]));
    expect(host.textContent).toContain("No proposals from Claude yet.");
  });

  it("名前、だれが、いつ、範囲、操作を出す", async () => {
    await mount(fakeApi([proposal(1, { responds_to: "c-9" })]));
    const text = host.textContent ?? "";
    expect(text).toContain("Proposal 1");
    expect(text).toContain("claude via mcp");
    expect(text).toContain("3 min ago");
    expect(text).toContain("c-9");
    expect(text).toContain("Sequence s1 · 0:01.50–0:04.00 · 2 cuts");
    expect(text).toContain("Extract cut k1");
  });

  it("読み込みの失敗をパネルの上に出す", async () => {
    await mount(fakeApi([], { list: vi.fn().mockRejectedValue("no project") }));
    expect(host.querySelector(".mm-review-error")?.textContent).toBe("no project");
  });
});

describe("承認と却下", () => {
  it("全部オンなら Approve で ops は null", async () => {
    const api = fakeApi([proposal(1)]);
    await mount(api);
    await click(button("Approve"));
    expect(api.approve).toHaveBeenCalledWith(1, null);
  });

  it("一部だけオフにすると Approve 2 of 3 になり、選んだ index を送る", async () => {
    const api = fakeApi([proposal(1)]);
    await mount(api);
    await click(host.querySelectorAll('input[type="checkbox"]')[1]);
    await click(button("Approve 2 of 3"));
    expect(api.approve).toHaveBeenCalledWith(1, [0, 2]);
  });

  it("全部オフなら承認できない", async () => {
    await mount(fakeApi([proposal(1)]));
    for (const box of host.querySelectorAll('input[type="checkbox"]')) await click(box);
    expect(button("Approve 0 of 3")?.disabled).toBe(true);
  });

  it("conflict があると承認を無効にして ID を出す", async () => {
    await mount(fakeApi([proposal(1, { conflict: ["k1", "k2"] })]));
    expect(button("Approve")?.disabled).toBe(true);
    expect(host.textContent).toContain("Conflicts with later edits: k1, k2");
  });

  it("Reject を送り、失敗の理由は上に出る", async () => {
    const api = fakeApi([proposal(1)], { reject: vi.fn().mockRejectedValue("already decided") });
    await mount(api);
    await click(button("Reject"));
    expect(api.reject).toHaveBeenCalledWith(1);
    expect(host.querySelector(".mm-review-error")?.textContent).toBe("already decided");
  });
});

describe("プレビュー", () => {
  it("Proposed で選んだ ops を送り、チェックを変えると送り直し、Original で戻す", async () => {
    const api = fakeApi([proposal(1)]);
    await mount(api);
    await click(button("Proposed"));
    expect(api.preview).toHaveBeenLastCalledWith(1, null);
    await click(host.querySelectorAll('input[type="checkbox"]')[0]);
    expect(api.preview).toHaveBeenLastCalledWith(1, [1, 2]);
    await click(button("Original"));
    expect(api.preview).toHaveBeenLastCalledWith(null, null);
  });

  it("Proposed になれるのは 1 つのカードだけ", async () => {
    await mount(fakeApi([proposal(2), proposal(1)]));
    const proposed = [...host.querySelectorAll("button")].filter((b) => b.textContent === "Proposed");
    await click(proposed[0]);
    await click(proposed[1]);
    expect(proposed.map((b) => b.getAttribute("aria-pressed"))).toEqual(["false", "true"]);
  });

  it("タブで隠れたら Program を元のフレームに戻す", async () => {
    const api = fakeApi([proposal(1)]);
    act(() => render(<ReviewPanel api={api} nowMs={NOW} active />, host));
    await settle();
    await click(button("Proposed"));
    act(() => render(<ReviewPanel api={api} nowMs={NOW} active={false} />, host));
    await settle();
    expect(api.preview).toHaveBeenLastCalledWith(null, null);
    expect(button("Original")?.getAttribute("aria-pressed")).toBe("true");
  });

});

describe("プレビューの失敗と順序", () => {
  it("適用できなかったら理由を出し、Proposed を押していない表示に戻す", async () => {
    const api = fakeApi([proposal(1)], { preview: vi.fn().mockRejectedValue(new Error("conflict")) });
    await mount(api);
    await click(button("Proposed"));
    expect(host.querySelector(".mm-review-error")?.textContent).toBe("conflict");
    expect(button("Proposed")?.getAttribute("aria-pressed")).toBe("false");
  });

  it("切り替えは前の頼みが終わってから送る", async () => {
    let finish: () => void = () => undefined;
    const preview = vi.fn().mockImplementationOnce(() => new Promise<void>((ok) => (finish = ok)));
    preview.mockResolvedValue(undefined);
    await mount(fakeApi([proposal(1)], { preview }));
    await click(button("Proposed"));
    await click(button("Original"));
    expect(preview).toHaveBeenCalledTimes(1);
    await act(async () => finish());
    await settle();
    expect(preview).toHaveBeenLastCalledWith(null, null);
  });
});

describe("決定済み", () => {
  it("承認済みは Applied と Undo、取り消し済みは Undone で Undo なし", async () => {
    const applied = proposal(2, { state: "approved", applied: true, undoable: true });
    const undone = proposal(1, { state: "approved", applied: false, undoable: false });
    const api = fakeApi([applied, undone]);
    await mount(api);
    const statuses = [...host.querySelectorAll(".mm-review-status")].map((s) => s.textContent);
    expect(statuses).toEqual(["Applied", "Undone"]);
    expect([...host.querySelectorAll("button")].filter((b) => b.textContent === "Undo")).toHaveLength(1);
    await click(button("Undo"));
    expect(api.undo).toHaveBeenCalledWith(2);
  });

  it("却下したものは Reopen で保留に戻せる", async () => {
    const api = fakeApi([proposal(1, { state: "rejected" })]);
    await mount(api);
    await click(button("Reopen"));
    expect(api.reopen).toHaveBeenCalledWith(1);
  });

  it("見出しで畳める", async () => {
    await mount(fakeApi([proposal(1, { state: "rejected" })]));
    await click(button("Decided (1)"));
    expect(host.querySelector('[aria-label="Decided proposals"]')).toBeNull();
  });
});

describe("読み直し", () => {
  it("提案のイベントと refreshKey で一覧を読み直し、外れたら購読を外す", async () => {
    let fire: (c: { id: number; state: string }) => void = () => undefined;
    const off = vi.fn();
    const api = fakeApi([], {
      onChanged: vi.fn(async (h: (c: { id: number; state: string }) => void) => {
        fire = h;
        return off;
      }),
    });
    act(() => render(<ReviewPanel api={api} refreshKey={0} />, host));
    await settle();
    expect(api.list).toHaveBeenCalledTimes(1);
    await act(async () => fire({ id: 1, state: "pending" }));
    expect(api.list).toHaveBeenCalledTimes(2);
    act(() => render(<ReviewPanel api={api} refreshKey={1} />, host));
    await settle();
    expect(api.list).toHaveBeenCalledTimes(3);
    act(() => render(null, host));
    expect(off).toHaveBeenCalled();
  });

  it("重なった読み直しは、後から頼んだ方の結果を残す", async () => {
    let fire: (c: { id: number; state: string }) => void = () => undefined;
    let slow: (l: Proposal[]) => void = () => undefined;
    const list = vi
      .fn()
      .mockResolvedValueOnce([])
      .mockImplementationOnce(() => new Promise<Proposal[]>((ok) => (slow = ok)))
      .mockResolvedValueOnce([proposal(1, { state: "rejected" })]);
    const api = fakeApi([], { list, onChanged: vi.fn(async (h: (c: { id: number; state: string }) => void) => ((fire = h), () => undefined)) });
    await mount(api);
    await act(async () => fire({ id: 1, state: "rejected" }));
    await act(async () => fire({ id: 1, state: "rejected" }));
    await settle();
    await act(async () => slow([proposal(1)]));
    await settle();
    expect(host.querySelector(".mm-review-status")?.textContent).toBe("Rejected");
  });
});
