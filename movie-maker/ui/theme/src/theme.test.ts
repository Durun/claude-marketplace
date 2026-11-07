import { readFileSync } from "node:fs";
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

describe("パネルとシェルの CSS", () => {
  const files = [
    "../../panels/tasks/src/tasks.css",
    "../../panels/media/src/media.css",
    "../../panels/story/src/story.css",
    "../../panels/mixer/src/mixer.css",
    "../../shell/src/shell.css",
  ];

  it("寸法を直書きせず、theme の変数から引く", () => {
    for (const f of files) {
      const css = readFileSync(new URL(f, import.meta.url), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
      expect(css, f).not.toMatch(/\d+px/);
    }
  });

  it("使う色の変数は theme.json にある", () => {
    const names = new Set(Object.keys(theme.modes.light.color));
    for (const f of files) {
      const css = readFileSync(new URL(f, import.meta.url), "utf8");
      for (const [, name] of css.matchAll(/--mm-color-([a-z-]+)/g)) {
        expect(names.has(name ?? ""), `${f}: ${name}`).toBe(true);
      }
    }
  });
});

describe("パネルが使う寸法の変数", () => {
  it("他のパネル用の名前を借りず、theme.json の size にある", () => {
    const sizes = new Set(Object.keys(theme.shape.size));
    const css = readFileSync(new URL("../../panels/story/src/story.css", import.meta.url), "utf8");
    for (const [, name] of css.matchAll(/--mm-size-([a-z-]+)/g)) {
      expect(sizes.has(name ?? ""), name).toBe(true);
      expect(name).toMatch(/^(story-|focus-offset)/);
    }
  });
});

describe("hexToLinear", () => {
  it("白と黒を端に対応づける", () => {
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

  it("部品の寸法と線の太さを変数にする", () => {
    expect(css).toContain("--mm-size-left-column: 260px;");
    expect(css).toContain("--mm-stroke-focus: 2px;");
  });

  it("グラデーションを CSS に直す", () => {
    expect(css).toContain("--mm-gradient-task-title: linear-gradient(to right, #ffffff, #c6d3f7);");
  });
});
