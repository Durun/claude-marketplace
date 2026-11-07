import { useEffect, useState } from "preact/hooks";
import { GraphPanel } from "@mm/graph";
import { InspectorPanel } from "./InspectorPanel.tsx";
import { describeLayer } from "./model.ts";
import type { Asset, Layer, Op } from "@mm/sdk";

/** 再生位置を知らせる DOM のイベント。アプリの再生の操作列（`web/transport.js`）が時計の通知ごとに投げる。 */
export const PLAYHEAD_EVENT = "mm:playhead";
/** 再生位置を動かしてほしいときに投げる DOM のイベント（`detail.time` は秒）。`web/transport.js` が受けてシークする。 */
export const SEEK_EVENT = "mm:seek";

export interface Playhead {
  time: number;
  fps: number;
}

/**
 * 再生位置のイベントを受けて持つ。届くまでは null（Inspector はキーフレームナビゲーターを出さない）。
 * 止まっている間も同じ位置が繰り返し届くので、位置が変わったときだけ描き直す（描き直すと入力中の入力欄が元の値に戻る）。
 */
const usePlayhead = (): Playhead | null => {
  const [playhead, setPlayhead] = useState<Playhead | null>(null);
  useEffect(() => {
    const on = (e: Event) => {
      const next = (e as CustomEvent<Playhead>).detail;
      setPlayhead((prev) => (prev && prev.time === next.time && prev.fps === next.fps ? prev : next));
    };
    window.addEventListener(PLAYHEAD_EVENT, on);
    return () => window.removeEventListener(PLAYHEAD_EVENT, on);
  }, []);
  return playhead;
};

const seek = (time: number) => window.dispatchEvent(new CustomEvent(SEEK_EVENT, { detail: { time } }));

export interface KeyframePanelsProps {
  layer: Layer | null;
  assets?: readonly Asset[];
  onOp: (op: Op) => void;
}

/** 右の列の Inspector と、その下の Graph。Inspector で名前を押したパラメータを Graph に出す。 */
export const KeyframePanels = ({ layer, assets, onOp }: KeyframePanelsProps) => {
  const playhead = usePlayhead();
  const [path, setPath] = useState<string | null>(null);
  const row = layer && path ? describeLayer(layer).flatMap((s) => s.rows).find((r) => r.path === path) : undefined;
  return (
    <>
      <InspectorPanel
        layer={layer}
        assets={assets}
        onOp={onOp}
        time={playhead?.time}
        fps={playhead?.fps}
        onSeek={seek}
        selectedPath={path}
        onSelectParam={setPath}
      />
      {layer && (
        <GraphPanel
          target={row ? { layer: layer.id, path: row.path } : null}
          param={row?.param ?? null}
          label={row ? `Graph: ${row.label}` : "Graph"}
          time={playhead?.time}
          fps={playhead?.fps}
          onOp={onOp}
        />
      )}
    </>
  );
};
