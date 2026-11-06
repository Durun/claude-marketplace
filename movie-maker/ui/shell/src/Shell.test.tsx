// @vitest-environment jsdom
import { render } from "preact";
import { act } from "preact/test-utils";
import type { Layer } from "@mm/sdk";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LAYER_SELECTED_EVENT, Shell } from "./Shell.tsx";

const host = document.createElement("div");
document.body.append(host);
afterEach(() => render(null, host));

const t0 = { num: 0, den: 1 };
const layer = {
  id: "l1",
  start: t0,
  in: t0,
  out: t0,
  opacity: { value: { type: "number", value: 1 } },
  source: { kind: "solid", version: 1 },
} as Layer;

describe("Shell", () => {
  it("右の列の Inspector は選択のイベントでレイヤーを出し、値の変更を sendOp に渡す", () => {
    const sendOp = vi.fn();
    act(() => render(<Shell sendOp={sendOp} />, host));
    expect(host.querySelector(".mm-shell-right")?.textContent).toContain("No layer selected");
    act(() => {
      window.dispatchEvent(new CustomEvent(LAYER_SELECTED_EVENT, { detail: { layer } }));
    });
    const opacity = host.querySelector<HTMLInputElement>('.mm-shell-right [aria-label="opacity"]');
    act(() => {
      if (opacity) opacity.value = "0.5";
      opacity?.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(sendOp).toHaveBeenCalledWith({
      op: "setParam",
      target: { layer: "l1", path: "opacity" },
      value: { type: "number", value: 0.5 },
    });
  });
});
