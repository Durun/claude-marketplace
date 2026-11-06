import { describe, expect, it } from "vitest";
import { baseName, formatBytes, formatDuration } from "./format.ts";
import { IDLE_PROGRESS, reduceProgress } from "./progress.ts";
import { envelopePath } from "./Waveform.tsx";

describe("format", () => {
  it("長さを m:ss と h:mm:ss にする", () => {
    expect(formatDuration(0)).toBe("0:00");
    expect(formatDuration(75.4)).toBe("1:15");
    expect(formatDuration(3725)).toBe("1:02:05");
  });

  it("バイト数を単位つきにする", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(5 * 1024 ** 3)).toBe("5.0 GB");
  });

  it("パスの末尾を取る", () => {
    expect(baseName("C:\\Videos\\a.mp4")).toBe("a.mp4");
    expect(baseName("/home/x/b.wav")).toBe("b.wav");
  });
});

describe("reduceProgress", () => {
  it("最初の素材が始まると前の失敗を捨て、終わりで結果を残す", () => {
    const failed = reduceProgress(IDLE_PROGRESS, { type: "failed", index: 0, path: "x", error: "e" });
    expect(failed.failures).toHaveLength(1);
    const second = reduceProgress(failed, { type: "started", index: 1, total: 2, path: "y" });
    expect(second.failures).toHaveLength(1);
    const next = reduceProgress(second, { type: "started", index: 0, total: 1, path: "z" });
    expect(next.failures).toHaveLength(0);
    const done = reduceProgress(next, { type: "finished", added: 1, skipped: 0, failed: 0 });
    expect(done).toMatchObject({ running: false, current: null, summary: { added: 1 } });
  });
});

describe("envelopePath", () => {
  it("中心から上下に振れた閉じた図形にする", () => {
    expect(envelopePath([0.5])).toBe("M0,1L0.5,0.5L1,1L0.5,1.5Z");
  });
});
