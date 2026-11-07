import type { MixerMeters } from "./api.ts";
import { describe, expect, it } from "vitest";
import { DECAY_DB_PER_S, HOLD_MS, MeterEngine } from "./meterEngine.ts";

const meters = (peak: number | null, playing = true): MixerMeters => ({
  t: 0,
  playing,
  tracks: [{ layer: "a", peak_db: peak }],
  master: { peak_db: [peak, peak], true_peak_db: null, short_term_lufs: null, integrated_lufs: null },
});

describe("MeterEngine", () => {
  it("上がるのは即座で、下がるのは一定の速さ", () => {
    const e = new MeterEngine();
    e.frame(0);
    e.push(meters(-6));
    e.frame(16);
    expect(e.shownDb("t:a")).toBe(-6);
    e.push(meters(-60));
    e.frame(16 + 500);
    expect(e.shownDb("t:a")).toBeCloseTo(-6 - DECAY_DB_PER_S * 0.5, 5);
  });

  it("ピークホールドは 1.5 秒保ち、その後に下がる", () => {
    const e = new MeterEngine();
    e.frame(0);
    e.push(meters(-6));
    e.frame(10);
    e.push(meters(null));
    e.frame(10 + HOLD_MS);
    expect(e.holdDb("t:a")).toBe(-6);
    e.frame(10 + HOLD_MS + 1000);
    expect(e.holdDb("t:a")).toBeLessThan(-6);
  });

  it("playing: false を受けると全部が下端へ下がる", () => {
    const e = new MeterEngine();
    e.frame(0);
    e.push(meters(-10));
    e.frame(10);
    e.push(meters(-10, false));
    e.frame(10 + 5000);
    expect(e.shownDb("t:a")).toBe(-60);
  });
});
