// Review パネルをモックの API で動かすページ。スクリーンショット用で、アプリには載せない。
import { render } from "preact";
import type { Op, RationalTime } from "@mm/sdk";
import type { Proposal, ProposalOp, ProposalScope, ReviewApi } from "../src/api.ts";
import { followTheme } from "../../../shell/src/followTheme.ts";
import { ReviewPanel } from "../src/index.ts";
import "../../../shell/src/shell.css";

/** 相対時刻を固定して、撮るたびに同じ画面にする。 */
const NOW_MS = Date.UTC(2026, 10, 7, 12, 0, 0);
const MINUTE_MS = 60_000;
const HOUR_MS = 3_600_000;

const sec = (num: number, den = 100): RationalTime => ({ num, den });

const ops = (list: [Op, string[]][]): ProposalOp[] => list.map(([op, touches], index) => ({ index, op, touches }));

const cutScope = (cuts: string[], start: number, end: number): ProposalScope => ({
  sequences: [{ id: "s1", cuts, start, end }],
  layers: [],
  other: [],
});

const base = (id: number, over: Partial<Proposal>): Proposal => ({
  id,
  name: "",
  actor: "mcp",
  actor_name: "claude",
  at_ms: NOW_MS,
  responds_to: null,
  base_version: 4,
  state: "pending",
  decided_by: null,
  decided_at_ms: null,
  txn_id: null,
  approved_ops: null,
  applied: false,
  undoable: false,
  conflict: null,
  ops: [],
  scope: { sequences: [], layers: [], other: [] },
  ...over,
});

let proposals: Proposal[] = [
  base(4, {
    name: "Tighten the intro",
    at_ms: NOW_MS - 3 * MINUTE_MS,
    responds_to: "c-12",
    ops: ops([
      [{ op: "ripple", cut: "k1", edge: "out", by: sec(-40) }, ["k1"]],
      [{ op: "split", cut: "k2", at: sec(250), new_cut: "k3" }, ["k2", "k3"]],
      [{ op: "extract", cut: "k4" }, ["k4"]],
    ]),
    scope: cutScope(["k1", "k2"], 1.5, 4),
  }),
  base(3, {
    name: "Fade the title in",
    at_ms: NOW_MS - 20 * MINUTE_MS,
    conflict: ["k1"],
    ops: ops([
      [{ op: "setParam", target: { layer: "l1", path: "opacity" }, value: { type: "number", value: 0.5 } }, ["l1"]],
      [{ op: "trimLayer", layer: "l1", edge: "out", by: sec(-50) }, ["l1"]],
    ]),
    scope: { sequences: [{ id: "s1", cuts: ["k1"], start: 0, end: 1.2 }], layers: [{ comp: "main", id: "l1" }], other: ["m1"] },
  }),
  base(2, {
    name: "Remove the long pause",
    actor: "cli",
    at_ms: NOW_MS - 2 * HOUR_MS,
    state: "approved",
    decided_by: "human",
    decided_at_ms: NOW_MS - HOUR_MS,
    txn_id: 9,
    approved_ops: [0],
    applied: true,
    undoable: true,
    ops: ops([[{ op: "lift", cut: "k5" }, ["k5"]]]),
    scope: cutScope(["k5"], 8, 9.25),
  }),
  base(1, {
    name: "Reorder the outro",
    at_ms: NOW_MS - 26 * HOUR_MS,
    state: "rejected",
    decided_by: "human",
    decided_at_ms: NOW_MS - 25 * HOUR_MS,
    ops: ops([[{ op: "addMarker", composition: "main", marker: { id: "m2", name: "Outro", time: sec(1800) } }, ["m2"]]]),
    scope: { sequences: [], layers: [], other: ["m2"] },
  }),
];

const replace = (id: number, over: Partial<Proposal>): Proposal => {
  const next = proposals.map((p) => (p.id === id ? { ...p, ...over } : p));
  proposals = next;
  return next.find((p) => p.id === id) as Proposal;
};

const api: ReviewApi = {
  list: async () => proposals,
  approve: async (id, picked) => ({
    proposal: replace(id, { state: "approved", approved_ops: picked, applied: true, undoable: true, txn_id: 10 }),
    change: null,
  }),
  reject: async (id) => replace(id, { state: "rejected" }),
  reopen: async (id) => replace(id, { state: "pending" }),
  undo: async (id) => replace(id, { applied: false, undoable: false }),
  preview: async (id, picked) => console.debug("preview", id, picked),
  onChanged: async () => () => undefined,
};

followTheme(document.documentElement, location.search, matchMedia.bind(window));
const root = document.getElementById("root");
if (root) {
  root.style.boxSizing = "border-box";
  root.style.flexDirection = "column";
  root.style.padding = "10px";
  root.style.width = "300px";
  root.style.height = "100vh";
  render(<ReviewPanel api={api} nowMs={NOW_MS} />, root);
}
