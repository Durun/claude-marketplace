// @vitest-environment jsdom
import { render } from "preact";
import { act } from "preact/test-utils";
import type { RenderTarget } from "@mm/sdk";
import type { DeliverApi, DeliverOverview, ExportItem, ExportStatus } from "./api.ts";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DeliverPanel } from "./DeliverPanel.tsx";

const host = document.createElement("div");
document.body.append(host);
afterEach(() => render(null, host));

const target = (id: string, over: Partial<RenderTarget> = {}): RenderTarget => ({
  id,
  composition: "main",
  width: 1920,
  height: 1080,
  fps: { num: 30, den: 1 },
  codec: "h264",
  output_path: `out/${id}.mp4`,
  ...over,
});

const overview = (targets: RenderTarget[], over: Partial<DeliverOverview> = {}): DeliverOverview => ({
  targets,
  compositions: [
    { id: "main", duration: 42, width: 1920, height: 1080 },
    { id: "short", duration: 30, width: 1080, height: 1920 },
  ],
  sequences: [{ id: "cut", duration: 30 }],
  open_composition: "main",
  can_verify: true,
  ...over,
});

const item = (over: Partial<ExportItem> = {}): ExportItem => ({
  target: "a",
  state: "queued",
  frames_done: 0,
  frames_total: 0,
  fps: 0,
  elapsed: 0,
  eta: null,
  output_path: "out/a.mp4",
  bytes_written: 0,
  verification: null,
  error: null,
  ...over,
});

const settle = () => act(() => new Promise<void>((ok) => setTimeout(ok, 0)));
const click = (el: Element | null | undefined) =>
  act(() => {
    (el as HTMLElement).click();
  });
const byText = (selector: string, text: string) =>
  [...host.querySelectorAll(selector)].find((e) => e.textContent?.trim() === text);

const mount = async (data: DeliverOverview, over: Partial<DeliverApi> = {}) => {
  let push: (s: ExportStatus) => void = () => {};
  const api: DeliverApi = {
    overview: vi.fn().mockResolvedValue(data),
    status: vi.fn().mockResolvedValue({ running: false, items: [] }),
    applyOps: vi.fn().mockResolvedValue(undefined),
    start: vi.fn().mockResolvedValue(undefined),
    cancel: vi.fn().mockResolvedValue(undefined),
    reveal: vi.fn().mockResolvedValue(undefined),
    pickOutput: vi.fn().mockResolvedValue(null),
    onProgress: vi.fn().mockImplementation(async (h) => {
      push = h;
      return () => {};
    }),
    ...over,
  };
  act(() => render(<DeliverPanel api={api} />, host));
  await settle();
  return { api, push: (s: ExportStatus) => act(() => push(s)) };
};

const setValue = (el: Element | null, value: string) =>
  act(() => {
    (el as HTMLInputElement).value = value;
    el?.dispatchEvent(new Event("input", { bubbles: true }));
  });

describe("一覧", () => {
  it("名前、大きさ、fps、コーデック、範囲、出力先を出す", async () => {
    await mount(overview([target("yt", { loudness_lufs: -14 })]));
    const row = host.querySelector(".mm-deliver-row")?.textContent ?? "";
    expect(row).toContain("yt");
    expect(row).toContain("1920×1080 · 30 fps · h264");
    expect(row).toContain("0:00 – 0:42 (main)");
    expect(row).toContain("out/yt.mp4");
    expect(row).toContain("Loudness (LUFS): -14");
  });

  it("sequence を選ぶと、範囲の表示がその全長になる", async () => {
    await mount(overview([target("yt")]));
    const select = host.querySelector<HTMLSelectElement>("#mm-deliver-sequence");
    act(() => {
      (select as HTMLSelectElement).value = "cut";
      select?.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(host.querySelector(".mm-deliver-row")?.textContent).toContain("0:00 – 0:30 (sequence cut)");
  });
});

describe("編集", () => {
  it("プリセットから足す: 初期値の入力欄を出し、Add で addRenderTarget を送る", async () => {
    const { api } = await mount(overview([]));
    await click(byText("button", "Add target"));
    expect((host.querySelector("#mm-deliver-field-id") as HTMLInputElement).value).toBe("youtube-1080p");
    await click(byText("button", "Add"));
    await settle();
    const [ops] = vi.mocked(api.applyOps).mock.calls[0] ?? [];
    expect(ops).toEqual([
      { op: "addRenderTarget", target: target("youtube-1080p", { codec: "h264_nvenc", loudness_lufs: -14, output_path: "exports/youtube-1080p.mp4" }) },
    ]);
  });

  it("直す: 外して元の位置に足す操作を 1 回の applyOps で送る", async () => {
    const { api } = await mount(overview([target("a"), target("yt"), target("b")]));
    await click([...host.querySelectorAll("button")].filter((b) => b.textContent === "Edit")[1]);
    setValue(host.querySelector("#mm-deliver-field-width"), "1280");
    await click(byText("button", "Save"));
    await settle();
    expect(api.applyOps).toHaveBeenCalledTimes(1);
    expect(vi.mocked(api.applyOps).mock.calls[0]?.[0]).toEqual([
      { op: "removeRenderTarget", target: "yt" },
      { op: "addRenderTarget", target: target("yt", { width: 1280 }), index: 1 },
    ]);
  });

  it("入力の誤りは送らずに入力欄の横へ出す", async () => {
    const { api } = await mount(overview([target("yt")]));
    await click(byText("button", "Edit"));
    setValue(host.querySelector("#mm-deliver-field-fps"), "fast");
    await click(byText("button", "Save"));
    expect(api.applyOps).not.toHaveBeenCalled();
    expect(host.querySelector(".mm-deliver-field-error")?.textContent).toContain("Enter a rate");
  });

  it("消す: removeRenderTarget を送る。拒否された理由は画面に出す", async () => {
    const applyOps = vi.fn().mockRejectedValue(new Error("コンポジションが無い"));
    await mount(overview([target("yt")]), { applyOps });
    await click(byText("button", "Remove"));
    await settle();
    expect(applyOps).toHaveBeenCalledWith([{ op: "removeRenderTarget", target: "yt" }]);
    expect(host.querySelector("[role=alert]")?.textContent).toContain("コンポジションが無い");
  });
});

describe("書き出しのキュー", () => {
  it("積んだ順に書き出しを頼む。何も積んでいないときは押せない", async () => {
    const { api } = await mount(overview([target("a"), target("b")]));
    expect((byText("button", "Export") as HTMLButtonElement).disabled).toBe(true);
    await click(host.querySelector('input[aria-label="Queue b"]'));
    await click(host.querySelector('input[aria-label="Queue a"]'));
    expect(host.querySelector(".mm-deliver-order")?.textContent).toBeTruthy();
    await click(byText("button", "Export 2 targets"));
    expect(api.start).toHaveBeenCalledWith({ targets: ["b", "a"], sequence: undefined, verify: true });
  });

});

describe("書き出しの進み具合", () => {
  it("進み具合を出し、書き出し中は取り消せる", async () => {
    const { api, push } = await mount(overview([target("a")]));
    await push({
      running: true,
      items: [
        item({ state: "running", frames_done: 300, frames_total: 1200, fps: 120.5, eta: 7.4 }),
        item({ target: "b", output_path: "out/b.mp4" }),
      ],
    });
    const bar = host.querySelector("[role=progressbar]");
    expect(bar?.getAttribute("aria-valuenow")).toBe("25");
    expect(host.querySelector(".mm-deliver-item")?.textContent).toContain("300 / 1200 frames · 120.5 fps · 0:07 left");
    expect(host.querySelectorAll(".mm-deliver-item")[1]?.textContent).toContain("Waiting");
    await click(byText("button", "Cancel"));
    expect(api.cancel).toHaveBeenCalled();
  });

  it("終わったら検証の結果とフォルダーを開くボタンを出す", async () => {
    const { api, push } = await mount(overview([target("a")]));
    const verification = { video_codec: "h264", width: 1920, height: 1080, video_frames: 1260, video_seconds: 42, audio_seconds: 42 };
    await push({
      running: false,
      items: [item({ state: "done", frames_done: 1260, frames_total: 1260, fps: 140, elapsed: 9, bytes_written: 48_300_000, verification })],
    });
    expect(host.querySelector(".mm-deliver-queue")?.textContent).toContain("Verified: h264 1920×1080, 1260 frames, 0:42, audio 0:42");
    expect(host.querySelector(".mm-deliver-queue")?.textContent).toContain("46.1 MB");
    await click(byText("button", "Open folder"));
    expect(api.reveal).toHaveBeenCalledWith("out/a.mp4");
  });

  it("失敗は理由を出し、検証できないときは検証のチェックボックスを無効にする", async () => {
    const { push } = await mount(overview([target("a")], { can_verify: false }));
    expect((host.querySelector(".mm-deliver-check-line input") as HTMLInputElement).disabled).toBe(true);
    await push({ running: false, items: [item({ state: "failed", error: "ffprobe を起動できない" })] });
    expect(host.querySelector(".mm-deliver-item [role=alert]")?.textContent).toBe("ffprobe を起動できない");
  });

  it("開き直したときは、動いているキューの状態を読む", async () => {
    await mount(overview([target("a")]), {
      status: vi.fn().mockResolvedValue({ running: true, items: [item({ state: "running", frames_done: 1, frames_total: 2 })] }),
    });
    expect(host.querySelector(".mm-deliver-queue")?.textContent).toContain("Exporting");
  });
});

describe("レビューの指摘（編集）", () => {
  const pickPreset = (id: string) =>
    act(() => {
      const select = host.querySelector<HTMLSelectElement>('select[aria-label="Preset"]');
      (select as HTMLSelectElement).value = id;
      select?.dispatchEvent(new Event("change", { bubbles: true }));
    });

  it("Shorts のプリセットは、縦長のコンポジションを既定にする", async () => {
    await mount(overview([]));
    pickPreset("youtube-shorts");
    await click(byText("button", "Add target"));
    expect((host.querySelector("#mm-deliver-field-composition") as HTMLSelectElement).value).toBe("short");
  });

  it("入れ子の delivery を、実際のスキーマの入力欄（選択とチェック）として出し、選んだ値を送る", async () => {
    const { api } = await mount(overview([]));
    await click(byText("button", "Add target"));
    const subtitles = host.querySelector<HTMLSelectElement>('[id="mm-deliver-field-delivery.subtitles"]');
    expect([...(subtitles?.options ?? [])].map((o) => o.value)).toEqual(["", "none", "sidecar", "embedded"]);
    expect(host.querySelector<HTMLInputElement>('[id="mm-deliver-field-delivery.chapters"]')?.type).toBe("checkbox");
    act(() => {
      (subtitles as HTMLSelectElement).value = "sidecar";
      subtitles?.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await click(byText("button", "Add"));
    await settle();
    const [ops] = vi.mocked(api.applyOps).mock.calls[0] ?? [];
    expect(ops?.[0]).toMatchObject({ op: "addRenderTarget", target: { delivery: { subtitles: "sidecar" }, loudness_lufs: -14 } });
  });

  it("誤りは入力欄に結び付き、aria-invalid が立つ", async () => {
    await mount(overview([target("yt")]));
    await click(byText("button", "Edit"));
    setValue(host.querySelector("#mm-deliver-field-width"), "1081");
    await click(byText("button", "Save"));
    const width = host.querySelector("#mm-deliver-field-width");
    expect(width?.getAttribute("aria-invalid")).toBe("true");
    const described = width?.getAttribute("aria-describedby") ?? "";
    expect(host.querySelector(`[id="${described}"]`)?.textContent).toContain("even");
    expect(host.querySelector("#mm-deliver-field-height")?.getAttribute("aria-invalid")).toBe("false");
  });

  it("出力先の保存ダイアログにコーデックを渡す", async () => {
    const { api } = await mount(overview([target("yt", { codec: "prores_ks", output_path: "out/yt.mov" })]));
    await click(byText("button", "Edit"));
    await click(byText("button", "Browse…"));
    expect(api.pickOutput).toHaveBeenCalledWith("out/yt.mov", "prores_ks");
  });
});

describe("レビューの指摘（書き出しのキュー）", () => {
  it("書き出しの依頼が返るまで、もう一度押しても依頼は 1 回", async () => {
    let finish: () => void = () => {};
    const start = vi.fn().mockImplementation(() => new Promise<void>((ok) => (finish = ok)));
    await mount(overview([target("a")]), { start });
    await click(host.querySelector('input[aria-label="Queue a"]'));
    const button = byText("button", "Export 1 target") as HTMLButtonElement;
    await click(button);
    expect(button.disabled).toBe(true);
    await click(button);
    expect(start).toHaveBeenCalledTimes(1);
    finish();
    await settle();
    expect(button.disabled).toBe(false);
  });

  it("開いたときの状態の応答がイベントより後に届いても、新しい状態を上書きしない", async () => {
    let answer: (s: ExportStatus) => void = () => {};
    const status = vi.fn().mockImplementation(() => new Promise<ExportStatus>((ok) => (answer = ok)));
    const { push } = await mount(overview([target("a")]), { status });
    await push({ running: true, items: [item({ state: "running", frames_done: 5, frames_total: 10 })] });
    await act(async () => answer({ running: false, items: [] }));
    expect(host.querySelector(".mm-deliver-queue")?.textContent).toContain("Exporting");
  });

  it("購読を先に始めてから状態を読む", async () => {
    const order: string[] = [];
    await mount(overview([target("a")]), {
      onProgress: vi.fn().mockImplementation(async () => {
        order.push("subscribe");
        return () => {};
      }),
      status: vi.fn().mockImplementation(async () => {
        order.push("status");
        return { running: false, items: [] };
      }),
    });
    expect(order).toEqual(["subscribe", "status"]);
  });
});
