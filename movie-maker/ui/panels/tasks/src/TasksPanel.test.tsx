// @vitest-environment jsdom
import { render } from "preact";
import { act } from "preact/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_GROUPS } from "./groups.ts";
import { TasksPanel } from "./TasksPanel.tsx";

const host = document.createElement("div");
document.body.append(host);

afterEach(() => render(null, host));

const click = (el: Element | null | undefined) => {
  act(() => {
    (el as HTMLElement).click();
  });
};

describe("TasksPanel", () => {
  it("項目を押すと命令の名前を onInvoke に渡す", () => {
    const onInvoke = vi.fn();
    render(<TasksPanel onInvoke={onInvoke} />, host);
    click(host.querySelector(".mm-tasks-link"));
    expect(onInvoke).toHaveBeenCalledWith(DEFAULT_GROUPS[0]?.items[0]?.command);
  });

  it("disabled の項目は押せず、命令を呼ばない", () => {
    const onInvoke = vi.fn();
    const groups = [{ id: "g", title: "G", items: [{ id: "a", label: "A", command: "a.run", disabled: true }] }];
    render(<TasksPanel groups={groups} onInvoke={onInvoke} />, host);
    const link = host.querySelector<HTMLButtonElement>(".mm-tasks-link");
    expect(link?.disabled).toBe(true);
    click(link);
    expect(onInvoke).not.toHaveBeenCalled();
  });

  it("見出しを押すとまとまりを閉じて、もう一度で開く", () => {
    render(<TasksPanel onInvoke={vi.fn()} />, host);
    const toggle = host.querySelector(".mm-tasks-toggle");
    const total = host.querySelectorAll(".mm-tasks-link").length;
    click(toggle);
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    expect(host.querySelectorAll(".mm-tasks-link").length).toBe(total - (DEFAULT_GROUPS[0]?.items.length ?? 0));
    click(toggle);
    expect(host.querySelectorAll(".mm-tasks-link").length).toBe(total);
  });

  it("initiallyClosed のまとまりは閉じて始まる", () => {
    render(<TasksPanel onInvoke={vi.fn()} initiallyClosed={["import"]} />, host);
    expect(host.querySelector(".mm-tasks-toggle")?.getAttribute("aria-expanded")).toBe("false");
  });
});
