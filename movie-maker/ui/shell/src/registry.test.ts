import { describe, expect, it, vi } from "vitest";
import { createPanelApis, PANELS } from "./registry.ts";

const idsOf = (column: "left" | "right") => PANELS.filter((p) => p.column === column).map((p) => p.id);

describe("パネルの登録", () => {
  it("左の列は Tasks、Media、Story、Transition、Mixer、Deliver の順", () => {
    expect(idsOf("left")).toEqual(["tasks", "media", "story", "transition", "mixer", "deliver"]);
  });

  it("右の列は Inspector、Review の順", () => {
    expect(idsOf("right")).toEqual(["inspector", "review"]);
  });

  it("id は重ならず、同じ列の order も重ならない", () => {
    expect(new Set(PANELS.map((p) => p.id)).size).toBe(PANELS.length);
    for (const column of ["left", "right"] as const) {
      const orders = PANELS.filter((p) => p.column === column).map((p) => p.order);
      expect(new Set(orders).size).toBe(orders.length);
    }
  });

  it("createApi を持つパネルの API だけを id で作る", () => {
    const createApi = vi.fn(() => "api");
    const apis = createPanelApis([
      { id: "a", label: "A", column: "left", order: 1, createApi, render: () => null },
      { id: "b", label: "B", column: "left", order: 2, render: () => null },
    ]);
    expect(apis).toEqual({ a: "api" });
  });
});
