// Inspector のキーフレームナビゲーターと Graph を、手元のレイヤーで動かす確認用のページ。アプリには載せない。
// 送った操作は applyKeyframeOp で同じ画面に適用して見せる（本番は mm-ops が適用する）。
import { render } from "preact";
import { useState } from "preact/hooks";
import { applyKeyframeOp, type Keyframe, type Layer, type Op, type Param } from "@mm/sdk";
import { GraphPanel } from "@mm/graph";
import { InspectorPanel } from "../src/index.ts";
import "./demo.css";
import "./keyframes.css";

const FPS = 30;
const t = (num: number, den = 1) => ({ num, den });
const key = (time: number, value: Keyframe["value"], extra: Partial<Keyframe> = {}): Keyframe => ({
  time: t(time),
  value,
  interpolation: "linear",
  ...extra,
});
const n = (value: number) => ({ type: "number" as const, value });
const v2 = (x: number, y: number) => ({ type: "vec2" as const, value: [x, y] });

const INITIAL = {
  id: "title",
  start: t(0),
  in: t(0),
  out: t(4),
  opacity: { value: n(1), keyframes: [key(0, n(0), { interpolation: "bezier" }), key(1, n(1), { interpolation: "hold" }), key(3, n(0.4))] },
  transform: {
    position: {
      value: v2(0, 0),
      keyframes: [
        key(0, v2(-120, 40), { interpolation: "bezier", bezier: { x1: 0.2, y1: 0.9, x2: 0.4, y2: 1 } }),
        key(2, v2(60, -20), { ease: "in_out" }),
        key(3.5, v2(120, 0)),
      ],
    },
    scale: { value: n(1) },
    rotation: { value: n(0) },
  },
  source: { kind: "text", version: 1, params: {} },
} as unknown as Layer;

type Slot = { get: (l: Layer) => Param | undefined; set: (l: Layer, p: Param) => Layer };

const SLOTS: Record<string, Slot> = {
  opacity: { get: (l) => l.opacity, set: (l, p) => ({ ...l, opacity: p }) },
  ...Object.fromEntries(
    ["position", "scale", "rotation"].map((name) => [
      `transform.${name}`,
      {
        get: (l: Layer) => l.transform?.[name as "position"],
        set: (l: Layer, p: Param) => ({ ...l, transform: { ...l.transform, [name]: p } }),
      },
    ]),
  ),
};

const applyOp = (layer: Layer, op: Op): Layer => {
  if (!("target" in op) || typeof op.target !== "object" || !("path" in op.target)) return layer;
  const slot = SLOTS[op.target.path];
  const param = slot?.get(layer);
  if (!slot || !param) return layer;
  if (op.op === "setParam") return slot.set(layer, { ...param, value: op.value });
  return slot.set(layer, applyKeyframeOp(param, op));
};

const query = new URLSearchParams(location.search);

const Demo = () => {
  const [layer, setLayer] = useState(INITIAL);
  const [time, setTime] = useState(Number(query.get("t") ?? "2"));
  const [path, setPath] = useState<string | null>(query.get("param") ?? "transform.position");
  const [log, setLog] = useState<Op[]>([]);
  const onOp = (op: Op) => {
    setLayer((l) => applyOp(l, op));
    setLog((prev) => [op, ...prev]);
  };
  const param = path ? SLOTS[path]?.get(layer) : undefined;
  return (
    <div class="demo">
      <div class="demo-inspector">
        <InspectorPanel
          layer={layer}
          onOp={onOp}
          time={time}
          fps={FPS}
          onSeek={setTime}
          selectedPath={path}
          onSelectParam={setPath}
        />
      </div>
      <div class="demo-graph">
        <label class="demo-time">
          <span>time {time.toFixed(2)}s</span>
          <input type="range" min={0} max={4} step={1 / FPS} value={time} onInput={(e) => setTime(e.currentTarget.valueAsNumber)} />
        </label>
        <GraphPanel
          target={path ? { layer: layer.id, path } : null}
          param={param ?? null}
          label={path ? `Graph: ${path}` : "Graph"}
          time={time}
          fps={FPS}
          width={480}
          height={260}
          onOp={onOp}
        />
        <pre class="demo-log">{log.length === 0 ? "ops sent: (none)" : log.map((o) => JSON.stringify(o)).join("\n")}</pre>
      </div>
    </div>
  );
};

document.documentElement.dataset.theme = query.get("theme") === "dark" ? "dark" : "light";
const root = document.getElementById("root");
if (root) render(<Demo />, root);
