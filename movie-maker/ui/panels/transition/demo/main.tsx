// Transition パネルをモックの API で動かすページ。スクリーンショット用で、アプリには載せない。
// ?state=plain（トランジションなし）か ?state=placed（いくつか置いてある、既定）。
import { render } from "preact";
import type { CutTransition, TransitionApi, TransitionCut } from "../src/api.ts";
import { followTheme } from "../../../shell/src/followTheme.ts";
import { TransitionPanel } from "../src/index.ts";
import "../../../shell/src/shell.css";

const KINDS = ["boot", "dip", "dissolve", "filter", "flash", "glitch", "liquid", "whip"];
const LENGTHS = [4, 2.5, 6, 3, 5];

const placed = new URLSearchParams(location.search).get("state") !== "plain";
const cuts: TransitionCut[] = (() => {
  let at = 0;
  return LENGTHS.map((duration, i) => {
    const cut = { id: `k${i + 1}`, start: at, duration, transition: null as CutTransition | null };
    at += duration;
    return cut;
  });
})();
const find = (id: string): TransitionCut => {
  const cut = cuts.find((c) => c.id === id);
  if (!cut) throw new Error(`カットが無い: ${id}`);
  return cut;
};
if (placed) {
  find("k2").transition = { kind: "whip", duration: 0.7 };
  find("k4").transition = { kind: "liquid", duration: 1.2 };
}

const api: TransitionApi = {
  cuts: async () => structuredClone({ sequence: "main", cuts }),
  kinds: async () => KINDS,
  set: async (cut, kind, duration) => {
    find(cut).transition = { kind, duration };
  },
  remove: async (cut) => {
    find(cut).transition = null;
  },
};

followTheme(document.documentElement, location.search, matchMedia.bind(window));
const root = document.getElementById("root");
if (root) {
  root.style.boxSizing = "border-box";
  root.style.flexDirection = "column";
  root.style.padding = "10px";
  root.style.width = "300px";
  root.style.height = "100vh";
  render(<TransitionPanel api={api} refreshKey={0} />, root);
}
