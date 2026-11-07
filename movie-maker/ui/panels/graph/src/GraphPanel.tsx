import { useEffect, useRef, useState } from "preact/hooks";
import { type Keyframe, type Op, type Param, type ParamRef, keyIndexAt, numbersOf, toSec } from "@mm/sdk";
import {
  curvePath,
  dragHandle,
  dragKey,
  fitView,
  handlePoint,
  SIDES,
  type Side,
  keyValue,
  secOf,
  segmentBezier,
  valueOf,
  xOf,
  yOf,
  type View,
} from "./view.ts";

const DEFAULT_WIDTH = 360;
const DEFAULT_HEIGHT = 200;
const DEFAULT_FPS = 30;
const KEY_RADIUS = 5;
const HANDLE_RADIUS = 4;
const COMPONENT_NAMES = ["x", "y", "z"];
/** 読み上げの名前に出す秒の桁。 */
const LABEL_DIGITS = 3;

export interface GraphPanelProps {
  /** 描くパラメータの指し方。操作の `target` になる。 */
  target: ParamRef | null;
  param: Param | null;
  /** 見出しに出す名前。 */
  label?: string;
  /** 再生位置（コンポジションの秒）。縦の線で示す。 */
  time?: number;
  /** コンポジションの fps。キーフレームの時刻をフレームにそろえる。 */
  fps?: number;
  width?: number;
  height?: number;
  /** 離したときに 1 つの操作を渡す。送るのは呼び出し側。 */
  onOp: (op: Op) => void;
  /** ドラッグ中の見た目のパラメータ。離すと null。プレビューに映す呼び出し側が使う。 */
  onPreview?: (param: Param | null) => void;
}

type Grab = { kind: "key"; i: number; c: number } | { kind: "handle"; i: number; side: Side; c: number };

interface Drag {
  grab: Grab;
  view: View;
  startX: number;
  startY: number;
  base: Param;
  draft: Param;
}

const localPoint = (svg: SVGSVGElement | null, v: View, e: PointerEvent): [number, number] => {
  const r = svg?.getBoundingClientRect();
  const sx = r && r.width > 0 ? v.width / r.width : 1;
  const sy = r && r.height > 0 ? v.height / r.height : 1;
  return [(e.clientX - (r?.left ?? 0)) * sx, (e.clientY - (r?.top ?? 0)) * sy];
};

/** ドラッグ中の位置から、動かした後のキーフレームの並びを作る。 */
const dragged = (d: Drag, x: number, y: number, fps: number, shift: boolean): Keyframe[] | null => {
  const keys = d.base.keyframes ?? [];
  const { grab, view } = d;
  if (grab.kind === "key") {
    const constrain = shift ? { horizontal: Math.abs(x - d.startX) >= Math.abs(y - d.startY) } : undefined;
    const dSec = secOf(view, x) - secOf(view, d.startX);
    const dValue = valueOf(view, y) - valueOf(view, d.startY);
    const k = dragKey(keys, grab.i, grab.c, { dSec, dValue, fps, constrain });
    return k ? keys.map((old, j) => (j === grab.i ? k : old)) : null;
  }
  const b = dragHandle(keys, grab.i, grab.side, grab.c, { sec: secOf(view, x), value: valueOf(view, y) });
  return b ? keys.map((old, j) => (j === grab.i ? { ...old, interpolation: "bezier" as const, bezier: b } : old)) : null;
};

/** 離したときに送る操作。動いていなければ null。 */
const commitOp = (target: ParamRef, d: Drag): Op | null => {
  const before = d.base.keyframes?.[d.grab.i];
  const after = d.draft.keyframes?.[d.grab.i];
  if (!before || !after || JSON.stringify(before) === JSON.stringify(after)) return null;
  if (d.grab.kind === "key") return { op: "moveKeyframe", target, from: before.time, keyframe: after };
  return { op: "setEase", target, time: before.time, interpolation: "bezier", ease: before.ease, bezier: after.bezier };
};

const useDrag = (props: GraphPanelProps, svg: { current: SVGSVGElement | null }) => {
  const [drag, setDrag] = useState<Drag | null>(null);
  const fps = props.fps ?? DEFAULT_FPS;
  const start = (grab: Grab, view: View, e: PointerEvent) => {
    if (!props.param) return;
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    const [x, y] = localPoint(svg.current, view, e);
    setDrag({ grab, view, startX: x, startY: y, base: props.param, draft: props.param });
  };
  const move = (e: PointerEvent) => {
    if (!drag) return;
    const [x, y] = localPoint(svg.current, drag.view, e);
    const keys = dragged(drag, x, y, fps, e.shiftKey);
    if (!keys) return;
    const draft = { ...drag.base, keyframes: keys };
    setDrag({ ...drag, draft });
    props.onPreview?.(draft);
  };
  const end = () => {
    if (!drag) return;
    const op = props.target && commitOp(props.target, drag);
    setDrag(null);
    props.onPreview?.(null);
    if (op) props.onOp(op);
  };
  // Esc でドラッグを取りやめ、操作を送らずに元へ戻す（AE と Blender の Graph Editor と同じ）。
  const dragging = drag !== null;
  const { onPreview } = props;
  useEffect(() => {
    if (!dragging) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      setDrag(null);
      onPreview?.(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [dragging, onPreview]);
  return { drag, start, move, end };
};

interface LayerProps {
  param: Param;
  c: number;
  view: View;
  onGrab: (grab: Grab, e: PointerEvent) => void;
}

const Handles = ({ param, c, view, onGrab }: LayerProps) => {
  const keys = param.keyframes ?? [];
  return (
    <g class="mm-graph-handles">
      {keys.flatMap((k, i) => {
        const next = keys[i + 1];
        if (!next || !segmentBezier(k)) return [];
        return SIDES.map((side) => {
          const [hs, hv] = handlePoint(k, next, side, c);
          const anchor = side === "out" ? k : next;
          const [ax, ay] = [xOf(view, toSec(anchor.time)), yOf(view, keyValue(anchor, c))];
          const [hx, hy] = [xOf(view, hs), yOf(view, hv)];
          return (
            <g key={`${i}-${side}`}>
              <line class="mm-graph-handle-line" x1={ax} y1={ay} x2={hx} y2={hy} />
              <circle
                class="mm-graph-handle"
                cx={hx}
                cy={hy}
                r={HANDLE_RADIUS}
                role="button"
                aria-label={`handle ${i} ${side} ${COMPONENT_NAMES[c] ?? c}`}
                onPointerDown={(e) => onGrab({ kind: "handle", i, side, c }, e)}
              />
            </g>
          );
        });
      })}
    </g>
  );
};

const Keys = ({ param, c, view, onGrab, current }: LayerProps & { current: number }) => (
  <g class="mm-graph-keys">
    {(param.keyframes ?? []).map((k, i) => (
      <circle
        key={i}
        class="mm-graph-key"
        data-current={i === current}
        cx={xOf(view, toSec(k.time))}
        cy={yOf(view, keyValue(k, c))}
        r={KEY_RADIUS}
        role="button"
        aria-label={`key ${toSec(k.time).toFixed(LABEL_DIGITS)}s ${COMPONENT_NAMES[c] ?? c}`}
        onPointerDown={(e) => onGrab({ kind: "key", i, c }, e)}
      />
    ))}
  </g>
);

const Message = ({ label, text }: { label?: string; text: string }) => (
  <div class="mm-graph">
    {label && <h3 class="mm-graph-title">{label}</h3>}
    <p class="mm-graph-empty">{text}</p>
  </div>
);

/**
 * 選んだパラメータの値の曲線。キーフレームとベジェのハンドルをドラッグで動かす。
 * ドラッグ中は手元の見た目と onPreview に映し、離したら 1 つの操作（moveKeyframe か setEase）を渡す。
 */
export const GraphPanel = (props: GraphPanelProps) => {
  const { param, label, time } = props;
  const width = props.width ?? DEFAULT_WIDTH;
  const height = props.height ?? DEFAULT_HEIGHT;
  const svg = useRef<SVGSVGElement>(null);
  const { drag, start, move, end } = useDrag(props, svg);
  if (!param || !props.target) return <Message label={label} text="Select a parameter in the Inspector" />;
  const comps = numbersOf(param.value)?.length ?? 0;
  if (comps === 0) return <Message label={label} text="This parameter has no curve" />;
  const shown = drag?.draft ?? param;
  const view = drag?.view ?? fitView(param, comps, width, height);
  const onGrab = (grab: Grab, e: PointerEvent) => start(grab, view, e);
  // Inspector の ◆ と同じく、再生位置と同じフレームのキーフレームを再生位置のキーとして示す。
  const current = time === undefined ? -1 : keyIndexAt(shown, time, props.fps ?? DEFAULT_FPS);
  const ids = Array.from({ length: comps }, (_, c) => c);
  return (
    <div class="mm-graph">
      {label && <h3 class="mm-graph-title">{label}</h3>}
      <svg
        ref={svg}
        class="mm-graph-canvas"
        viewBox={`0 0 ${width} ${height}`}
        width={width}
        height={height}
        data-dragging={drag !== null}
        onPointerMove={move}
        onPointerUp={end}
        onPointerCancel={end}
      >
        <line class="mm-graph-zero" x1={0} x2={width} y1={yOf(view, 0)} y2={yOf(view, 0)} />
        {time !== undefined && <line class="mm-graph-playhead" x1={xOf(view, time)} x2={xOf(view, time)} y1={0} y2={height} />}
        {ids.map((c) => (
          <path key={c} class="mm-graph-curve" data-component={c} d={curvePath(shown, c, view)} />
        ))}
        {ids.map((c) => (
          <Handles key={c} param={shown} c={c} view={view} onGrab={onGrab} />
        ))}
        {ids.map((c) => (
          <Keys key={c} param={shown} c={c} view={view} onGrab={onGrab} current={current} />
        ))}
      </svg>
      {(param.keyframes?.length ?? 0) === 0 && <p class="mm-graph-empty">No keyframes. Add one in the Inspector.</p>}
    </div>
  );
};

