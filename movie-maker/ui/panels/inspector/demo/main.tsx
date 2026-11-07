// Inspector の確認用のページ。fixtures の JSON を読み、送った操作を同じ画面で適用して見せる。
import { render } from "preact";
import { useMemo, useState } from "preact/hooks";
import { createClient, type Asset, type Composition, type Layer, type Op, type Param, type Transport } from "@mm/sdk";
import { DEFAULT_HINTS, InspectorPanel, type ParamHints } from "../src/index.ts";
import project from "../../../../fixtures/ui/inspector.json" with { type: "json" };
import "./demo.css";

const HINTS: ParamHints = {
  ...DEFAULT_HINTS,
  "source.font": { choices: ["Noto Sans JP", "Tahoma", "Meiryo UI"] },
  "effects.fx1.edge": { choices: ["clamp", "mirror", "wrap"] },
};

const doc = project as unknown as { assets: Asset[]; compositions: Composition[] };

/** 持ち主の中のパス（`source.size`、`effects.fx1.radius` など）が指す Param の入れ物と名前。 */
const slot = (layer: Layer, path: string): [Record<string, Param>, string] | undefined => {
  const [head, ...rest] = path.split(".");
  const name = rest.join(".");
  if (head === "opacity") return [layer as unknown as Record<string, Param>, "opacity"];
  if (head === "transform") return [(layer.transform ??= {}) as Record<string, Param>, name];
  if (head === "audio" && layer.audio) return [layer.audio as unknown as Record<string, Param>, name];
  if (head === "crop") return [(layer.crop ??= {}) as Record<string, Param>, name];
  if (head === "masks") {
    const [id, , n] = rest;
    const rect = layer.masks?.find((m) => m.id === id)?.rect;
    return rect ? [rect as unknown as Record<string, Param>, n ?? ""] : undefined;
  }
  if (head === "source") return [(layer.source.params ??= {}), name];
  if (head === "effects") {
    const [id, ...n] = rest;
    const fx = layer.effects?.find((e) => e.id === id);
    return fx ? [(fx.node.params ??= {}), n.join(".")] : undefined;
  }
  return undefined;
};

const apply = (layer: Layer, op: Op): void => {
  if (op.op === "addMask") layer.masks = [...(layer.masks ?? []), op.mask];
  if (op.op === "removeMask") layer.masks = layer.masks?.filter((m) => m.id !== op.mask);
  if (op.op === "setLayerAudio") layer.audio = op.audio;
  if (op.op !== "setParam") return;
  const found = slot(layer, op.target.path);
  const param = found?.[0][found[1]];
  if (found && param) found[0][found[1]] = { ...param, value: op.value };
};

const targetLayer = (op: Op, layers: readonly Layer[]): string | null | undefined => {
  if (op.op === "setParam") return op.target.layer;
  if (op.op === "addMask" || op.op === "setLayerAudio") return op.layer;
  if (op.op === "removeMask") return layers.find((l) => l.masks?.some((m) => m.id === op.mask))?.id;
  return null;
};

const Demo = () => {
  const layers = doc.compositions[0]?.layers ?? [];
  const [layerId, setLayerId] = useState(layers[0]?.id ?? "");
  const [, bump] = useState(0);
  const [log, setLog] = useState<Op[]>([]);
  const layer = layers.find((l) => l.id === layerId) ?? null;

  const client = useMemo(() => {
    const transport: Transport = {
      send: async (op) => {
        const target = layers.find((l) => l.id === targetLayer(op, layers));
        if (target) apply(target, op);
        setLog((prev) => [op, ...prev]);
        bump((n) => n + 1);
        return { ok: true, value: null };
      },
    };
    return createClient(transport);
  }, [layers]);

  return (
    <div class="demo">
      <div class="demo-inspector">
        <select
          class="demo-select"
          aria-label="layer"
          value={layerId}
          onChange={(e) => setLayerId(e.currentTarget.value)}
        >
          {layers.map((l) => (
            <option key={l.id} value={l.id}>
              {l.id}
            </option>
          ))}
        </select>
        <InspectorPanel layer={layer} assets={doc.assets} hints={HINTS} onOp={(op) => void client.sendOp(op)} />
      </div>
      <pre class="demo-log">{log.length === 0 ? "ops sent: (none)" : log.map((o) => JSON.stringify(o)).join("\n")}</pre>
    </div>
  );
};

const params = new URLSearchParams(location.search);
document.documentElement.dataset.theme = params.get("theme") === "dark" ? "dark" : "light";
const root = document.getElementById("root");
if (root) render(<Demo />, root);
