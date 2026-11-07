import type { Op } from "@mm/sdk";
import type { ProposalOp } from "./api.ts";
import { describe, expect, it } from "vitest";
import { approveLabel, clock, describeOp, describeScope, relativeTime } from "./model.ts";

const line = (op: Op, touches: string[] = []): string => describeOp({ index: 0, op, touches } as ProposalOp);
const t = (num: number, den = 100) => ({ num, den });

describe("describeOp", () => {
  it("リップルは端と秒数を符号つきで書く", () => {
    expect(line({ op: "ripple", cut: "k1", edge: "out", by: t(-40) })).toBe("Ripple cut k1 out edge by −0.40s");
    expect(line({ op: "ripple", cut: "k1", edge: "in", by: t(1, 4) })).toBe("Ripple cut k1 in edge by +0.25s");
  });

  it("setParam は持ち主と値を書く", () => {
    const op: Op = { op: "setParam", target: { layer: "l1", path: "opacity" }, value: { type: "number", value: 0.5 } };
    expect(line(op)).toBe("Set opacity on layer l1 to 0.5");
  });

  it("カットの操作を 1 行にする", () => {
    expect(line({ op: "split", cut: "k1", at: t(250), new_cut: "k2" })).toBe("Split cut k1 at 2.50s (new cut k2)");
    expect(line({ op: "extract", cut: "k1" })).toBe("Extract cut k1");
    expect(line({ op: "lift", cut: "k1" })).toBe("Lift cut k1");
    expect(line({ op: "disable", cut: "k1", disabled: false })).toBe("Enable cut k1");
  });

  it("レイヤーとマーカーの操作を 1 行にする", () => {
    expect(line({ op: "removeLayer", layer: "l1" })).toBe("Remove layer l1");
    expect(line({ op: "trimLayer", layer: "l1", edge: "out", by: t(-50) })).toBe("Trim layer l1 out edge by −0.50s");
    expect(line({ op: "addMarker", composition: "main", marker: { id: "m1", name: "Beat", time: t(300) } })).toBe(
      'Add marker "Beat" at 3.00s',
    );
  });

  it("知らない操作は名前と触る ID の汎用形にする", () => {
    expect(line({ op: "rate", asset: "a1", rating: 3 } as unknown as Op, ["a1"])).toBe("rate (a1)");
    expect(line({ op: "rate" } as unknown as Op)).toBe("rate");
  });
});

describe("describeScope", () => {
  it("シーケンス、レイヤー、その他の ID を行にする", () => {
    expect(
      describeScope({
        sequences: [{ id: "s1", cuts: ["k1", "k2"], start: 1.5, end: 4 }],
        layers: [{ comp: "main", id: "l1" }],
        other: ["m1"],
      }),
    ).toEqual(["Sequence s1 · 0:01.50–0:04.00 · 2 cuts", "Layer l1 (main)", "Other: m1"]);
  });
});

describe("表示の部品", () => {
  it("秒を m:ss.cc にする", () => {
    expect(clock(65.25)).toBe("1:05.25");
  });

  it("相対時刻", () => {
    const now = 10_000_000_000;
    expect(relativeTime(now - 5_000, now)).toBe("just now");
    expect(relativeTime(now - 180_000, now)).toBe("3 min ago");
    expect(relativeTime(now - 7_200_000, now)).toBe("2 h ago");
    expect(relativeTime(now - 2 * 86_400_000, now)).toBe("2 d ago");
  });

  it("承認ボタンの文字", () => {
    expect(approveLabel(3, 3)).toBe("Approve");
    expect(approveLabel(2, 3)).toBe("Approve 2 of 3");
  });
});
