import { describe, expect, it } from "vitest";
import { MODE_NAMES, generateCss, hexToLinear, theme } from "./index.ts";

const HEX = /^#[0-9a-f]{6}$/i;

describe("theme.json", () => {
  it("ライトとダークが同じ名前の色とグラデーションを持つ", () => {
    const [a, b] = MODE_NAMES.map((m) => theme.modes[m]);
    expect(Object.keys(a?.color ?? {}).sort()).toEqual(Object.keys(b?.color ?? {}).sort());
    expect(Object.keys(a?.gradient ?? {}).sort()).toEqual(Object.keys(b?.gradient ?? {}).sort());
  });

  it("色は全部 #rrggbb で、CSS の関数に依存しない", () => {
    for (const m of MODE_NAMES) {
      const { color, gradient } = theme.modes[m];
      const all = [...Object.values(color), ...Object.values(gradient).flatMap((g) => [g.from, g.to])];
      for (const c of all) expect(c).toMatch(HEX);
    }
  });
});

describe("hexToLinear", () => {
  it("白と黒を端に写す", () => {
    expect(hexToLinear("#ffffff")).toEqual([1, 1, 1]);
    expect(hexToLinear("#000000")).toEqual([0, 0, 0]);
  });

  it("sRGB の中間を線形に直す", () => {
    const [r] = hexToLinear("#808080");
    expect(r).toBeCloseTo(0.2159, 3);
  });

  it("不正な形はエラーにする", () => {
    expect(() => hexToLinear("red")).toThrow();
  });
});

describe("generateCss", () => {
  const css = generateCss();

  it("両方の配色を data-theme で出す", () => {
    expect(css).toContain(':root[data-theme="dark"]');
    expect(css).toContain("--mm-color-bg: #ece9d8;");
    expect(css).toContain("--mm-color-bg: #1f2430;");
  });

  it("形の値を px で出し、影を含まない", () => {
    expect(css).toContain("--mm-radius-md: 4px;");
    expect(css).not.toMatch(/shadow/i);
  });

  it("グラデーションを CSS に直す", () => {
    expect(css).toContain("--mm-gradient-task-title: linear-gradient(to right, #ffffff, #c6d3f7);");
  });
});
