// @vitest-environment jsdom
import { render } from "preact";
import { act } from "preact/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createIpcTransitionApi, type TransitionApi, type TransitionState } from "./api.ts";
import { DEFAULT_DURATION, TransitionPanel } from "./TransitionPanel.tsx";

const host = document.createElement("div");
document.body.append(host);
afterEach(() => render(null, host));

const settle = () => act(() => new Promise<void>((ok) => setTimeout(ok, 0)));

const state = (): TransitionState => ({
  sequence: "s1",
  cuts: [
    { id: "k1", start: 0, duration: 4, transition: null },
    { id: "k2", start: 4, duration: 3, transition: { kind: "whip", duration: 0.7 } },
    { id: "k3", start: 7, duration: 2, transition: null },
  ],
});

const mount = async (over: Partial<TransitionApi> = {}) => {
  const api: TransitionApi = {
    cuts: vi.fn().mockResolvedValue(state()),
    kinds: vi.fn().mockResolvedValue(["dip", "dissolve", "whip"]),
    set: vi.fn().mockResolvedValue(undefined),
    remove: vi.fn().mockResolvedValue(undefined),
    ...over,
  };
  act(() => render(<TransitionPanel api={api} />, host));
  await settle();
  return api;
};

const cutButton = (id: string) =>
  [...host.querySelectorAll<HTMLButtonElement>(".mm-tr-cut")].find((b) => b.querySelector(".mm-tr-cut-id")?.textContent === id) as HTMLButtonElement;
const kind = () => host.querySelector<HTMLSelectElement>('[aria-label="Transition kind"]') as HTMLSelectElement;
const length = () => host.querySelector<HTMLInputElement>('[aria-label="Transition length"]') as HTMLInputElement;

const change = async (el: HTMLSelectElement | HTMLInputElement, value: string) => {
  act(() => {
    el.value = value;
    el.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await settle();
};

describe("TransitionPanel の表示", () => {
  it("カットを並べ、トランジションのあるカットの頭に印と種類を出す", async () => {
    await mount();
    expect([...host.querySelectorAll(".mm-tr-cut-id")].map((e) => e.textContent)).toEqual(["k1", "k2", "k3"]);
    expect(cutButton("k2").querySelector(".mm-tr-mark")?.getAttribute("aria-label")).toBe("Transition whip 0.70s");
    expect(cutButton("k2").querySelector(".mm-tr-cut-kind")?.textContent).toBe("whip");
    expect(cutButton("k3").querySelector(".mm-tr-mark")).toBeNull();
  });

  it("最初のカットはトランジションを選べない", async () => {
    await mount();
    expect(host.textContent).toContain("The first cut has no previous cut.");
    expect(kind()).toBeNull();
  });

  it("選んだカットの種類と長さを出す", async () => {
    await mount();
    act(() => cutButton("k2").click());
    expect(kind().value).toBe("whip");
    expect(length().value).toBe("0.7");
    act(() => cutButton("k3").click());
    expect(kind().value).toBe("");
    expect(length().disabled).toBe(true);
  });

});

describe("TransitionPanel の操作", () => {
  it("種類を選ぶと、置く操作を 1 回だけ送り、読み直す", async () => {
    const api = await mount();
    act(() => cutButton("k3").click());
    await change(kind(), "dip");
    expect(api.set).toHaveBeenCalledTimes(1);
    expect(api.set).toHaveBeenCalledWith("k3", "dip", DEFAULT_DURATION);
    expect(api.cuts).toHaveBeenCalledTimes(2);
  });

  it("種類を変えると、いまの長さのまま差し替える", async () => {
    const api = await mount();
    act(() => cutButton("k2").click());
    await change(kind(), "dissolve");
    expect(api.set).toHaveBeenCalledWith("k2", "dissolve", 0.7);
  });

  it("長さを変えると、いまの種類のまま 1 回送る", async () => {
    const api = await mount();
    act(() => cutButton("k2").click());
    await change(length(), "1.2");
    expect(api.set).toHaveBeenCalledTimes(1);
    expect(api.set).toHaveBeenCalledWith("k2", "whip", 1.2);
  });

  it("長さが 0 以下なら送らない", async () => {
    const api = await mount();
    act(() => cutButton("k2").click());
    await change(length(), "0");
    expect(api.set).not.toHaveBeenCalled();
  });

  it("None を選ぶと外す操作を 1 回送る", async () => {
    const api = await mount();
    act(() => cutButton("k2").click());
    await change(kind(), "");
    expect(api.remove).toHaveBeenCalledTimes(1);
    expect(api.remove).toHaveBeenCalledWith("k2");
    expect(api.set).not.toHaveBeenCalled();
  });

});

describe("TransitionPanel の失敗と例外", () => {
  it("拒否されたら理由を出す", async () => {
    await mount({ set: vi.fn().mockRejectedValue(new Error("長さは正にする")) });
    act(() => cutButton("k3").click());
    await change(kind(), "dip");
    expect(host.querySelector(".mm-tr-error")?.textContent).toBe("長さは正にする");
  });

  it("パックに無いトランジションは custom と出す", async () => {
    await mount({
      cuts: vi.fn().mockResolvedValue({
        sequence: "s1",
        cuts: [
          { id: "k1", start: 0, duration: 4, transition: null },
          { id: "k2", start: 4, duration: 3, transition: { kind: null, duration: 1 } },
        ],
      }),
    });
    act(() => cutButton("k2").click());
    expect(kind().selectedOptions[0]?.textContent).toBe("custom");
    expect(length().disabled).toBe(true);
  });

  it("sequence が無いときはその旨を出す", async () => {
    await mount({ cuts: vi.fn().mockResolvedValue({ sequence: null, cuts: [] }) });
    expect(host.textContent).toContain("No sequence in the open composition.");
  });
});

describe("createIpcTransitionApi", () => {
  it("コマンドの名前と引数を渡す", async () => {
    const call = vi.fn().mockResolvedValue(undefined);
    const api = createIpcTransitionApi(call, "pk");
    await api.cuts();
    await api.kinds();
    await api.set("k2", "whip", 0.7);
    await api.remove("k2");
    expect(call.mock.calls).toEqual([
      ["transition_cuts", { dir: "pk" }],
      ["pack_transitions", { dir: "pk" }],
      ["pack_apply", { dir: "pk", name: "transition", args: { cut: "k2", kind: "whip", dur: "0.7" } }],
      ["apply_op", { op: { op: "setTransition", cut: "k2", transition: null } }],
    ]);
  });

  it("外す操作が拒否されたら reject する", async () => {
    const call = vi.fn().mockRejectedValue("カットが無い");
    await expect(createIpcTransitionApi(call).remove("x")).rejects.toThrow("カットが無い");
  });
});
