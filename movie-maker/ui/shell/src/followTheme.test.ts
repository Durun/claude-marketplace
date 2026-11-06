// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { followTheme } from "./followTheme.ts";

/** OS の配色の切り替えを手で起こせる matchMedia。 */
const fakeMedia = (dark: boolean) => {
  const listeners = new Set<() => void>();
  const query = {
    matches: dark,
    addEventListener: (_: string, fn: () => void) => listeners.add(fn),
  };
  const media = (() => query) as unknown as typeof matchMedia;
  const set = (next: boolean) => {
    query.matches = next;
    listeners.forEach((fn) => fn());
  };
  return { media, set };
};

describe("followTheme", () => {
  it("OS の配色で始め、OS が切り替わったら追従する", () => {
    const root = document.createElement("html");
    const { media, set } = fakeMedia(false);
    followTheme(root, "", media);
    expect(root.dataset.theme).toBe("light");
    set(true);
    expect(root.dataset.theme).toBe("dark");
    set(false);
    expect(root.dataset.theme).toBe("light");
  });

  it("?theme= で固定したら OS の切り替えに追従しない", () => {
    const root = document.createElement("html");
    const { media, set } = fakeMedia(false);
    followTheme(root, "?theme=dark", media);
    set(false);
    set(true);
    expect(root.dataset.theme).toBe("dark");
  });
});
