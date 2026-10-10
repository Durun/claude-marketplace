import type { RefObject } from 'preact';
import { useEffect, useRef, useState } from 'preact/hooks';
import { clock, dateTime, dateTimeSeconds, fmtInt, fmtTokens } from './format.ts';

export type TimelinePoint = [at: number, tokens: number];

const HEIGHT = 260;
const MARGIN = { top: 18, right: 28, bottom: 28, left: 54 };
const Y_TICKS = 5;
const X_TICKS = 5;
const DAY_MS = 24 * 60 * 60 * 1000;
const TIP_WIDTH = 190;
const TIP_GAP = 12;
const DOT_RADIUS = 4;
// 末尾の値ラベルは点の上に置く。直前の点が高いとき（下降）は線と重なるため点の下に置く。
const LABEL_ABOVE = -10;
const LABEL_BELOW = 18;
const LIMIT_LABEL_RISE = 6;

const useWidth = (ref: RefObject<HTMLElement>): number => {
  const [width, setWidth] = useState(0);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    // 幅が変わると図の大きさも変わるので、次のフレームで描き直す。通知の中で描き直すと、ResizeObserver が通知を配りきれずにエラーを出す。
    const observer = new ResizeObserver(([entry]) => {
      const width = Math.floor(entry?.contentRect.width ?? 0);
      requestAnimationFrame(() => setWidth(width));
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return width;
};

/** 1・2・5 の倍数のうち、max を count 本前後に割れる刻み幅。 */
const niceStep = (max: number, count: number): number => {
  const raw = max / count;
  const base = 10 ** Math.floor(Math.log10(raw));
  return ([1, 2, 5, 10].find((unit) => raw / base <= unit) ?? 10) * base;
};

type Geometry = {
  x: (at: number) => number;
  y: (tokens: number) => number;
  yMax: number;
  yTicks: number[];
  xTicks: number[];
  spansDays: boolean;
};

const makeGeometry = (points: TimelinePoint[], limit: number, width: number): Geometry => {
  const times = points.map((p) => p[0]);
  const t0 = Math.min(...times);
  const t1 = Math.max(...times);
  const yMax = Math.max(limit, ...points.map((p) => p[1]), 1);
  const iw = Math.max(width - MARGIN.left - MARGIN.right, 1);
  const ih = HEIGHT - MARGIN.top - MARGIN.bottom;
  const step = niceStep(yMax, Y_TICKS);
  return {
    x: (at) => MARGIN.left + (t1 === t0 ? iw / 2 : ((at - t0) / (t1 - t0)) * iw),
    y: (tokens) => MARGIN.top + (1 - tokens / yMax) * ih,
    yMax,
    yTicks: Array.from({ length: Math.floor(yMax / step) + 1 }, (_, i) => i * step),
    xTicks: t1 === t0 ? [t0] : Array.from({ length: X_TICKS }, (_, i) => t0 + ((t1 - t0) * i) / (X_TICKS - 1)),
    spansDays: t1 - t0 >= DAY_MS,
  };
};

const linePath = (points: TimelinePoint[], g: Geometry): string =>
  points.map(([at, tokens], i) => `${i === 0 ? 'M' : 'L'}${g.x(at).toFixed(1)},${g.y(tokens).toFixed(1)}`).join('');

const nearestIndex = (points: TimelinePoint[], g: Geometry, px: number): number => {
  let best = 0;
  let bestDistance = Infinity;
  points.forEach(([at], i) => {
    const distance = Math.abs(g.x(at) - px);
    if (distance < bestDistance) {
      best = i;
      bestDistance = distance;
    }
  });
  return best;
};

type Plot = { points: TimelinePoint[]; limit: number; g: Geometry; width: number };

const Axes = ({ g, width, limit }: Pick<Plot, 'g' | 'width' | 'limit'>) => {
  const right = width - MARGIN.right;
  const base = g.y(0);
  return (
    <g>
      {g.yTicks.map((v) => (
        <g key={v}>
          <line class={v === 0 ? 'axis' : 'grid'} x1={MARGIN.left} x2={right} y1={g.y(v)} y2={g.y(v)} />
          <text class="axis-text" x={MARGIN.left - 8} y={g.y(v) + 4} text-anchor="end">{fmtTokens(v)}</text>
        </g>
      ))}
      {g.xTicks.map((t) => (
        <text key={t} class="axis-text" x={g.x(t)} y={base + 18} text-anchor="middle">
          {g.spansDays ? dateTime(t) : clock(t)}
        </text>
      ))}
      <line class="limit-line" x1={MARGIN.left} x2={right} y1={g.y(limit)} y2={g.y(limit)} />
      <text class="limit-label" x={right} y={g.y(limit) - LIMIT_LABEL_RISE} text-anchor="end">上限 {fmtInt(limit)}</text>
    </g>
  );
};

const Series = ({ points, g }: Pick<Plot, 'points' | 'g'>) => {
  const line = linePath(points, g);
  const first = points[0];
  const last = points[points.length - 1];
  const prev = points[points.length - 2];
  if (!first || !last) return null;
  const falling = prev !== undefined && prev[1] > last[1];
  return (
    <g>
      <path class="series-area" d={`${line}L${g.x(last[0])},${g.y(0)}L${g.x(first[0])},${g.y(0)}Z`} />
      <path class="series-line" d={line} />
      <circle class="series-dot" cx={g.x(last[0])} cy={g.y(last[1])} r={DOT_RADIUS} />
      <text
        class="end-label"
        x={g.x(last[0]) + DOT_RADIUS}
        y={g.y(last[1]) + (falling ? LABEL_BELOW : LABEL_ABOVE)}
        text-anchor="end"
      >
        {fmtTokens(last[1])}
      </text>
    </g>
  );
};

const Tooltip = ({ points, index, limit, g, width }: Plot & { index: number }) => {
  const point = points[index];
  if (!point) return null;
  const before = points[index - 1];
  const x = g.x(point[0]);
  const left = x + TIP_GAP + TIP_WIDTH > width ? x - TIP_GAP - TIP_WIDTH : x + TIP_GAP;
  return (
    <div class="chart-tip" style={`left: ${left}px; top: ${MARGIN.top}px; width: ${TIP_WIDTH}px`}>
      <strong>{fmtInt(point[1])} トークン</strong>
      <span>{((point[1] / limit) * 100).toFixed(1)}% · {dateTimeSeconds(point[0])}</span>
      {before && before[1] > point[1] && <span>直前の点から −{fmtInt(before[1] - point[1])}</span>}
    </div>
  );
};

/** コンテキストのトークン数の推移。上限を基準線にし、圧縮で下がる様子が線の落ち込みで見える。 */
export const Timeline = ({ points, limit }: { points: TimelinePoint[]; limit: number }) => {
  const box = useRef<HTMLDivElement>(null);
  const width = useWidth(box);
  const [hover, setHover] = useState<number | null>(null);
  const plot: Plot | null = points.length > 0 && width > 0 ? { points, limit, g: makeGeometry(points, limit, width), width } : null;
  const hovered = plot && hover !== null ? points[hover] : undefined;
  return (
    <div class="chart" ref={box}>
      {plot && (
        <svg
          width={width}
          height={HEIGHT}
          role="img"
          aria-label={`コンテキストのトークン数の推移。最新 ${fmtInt(points[points.length - 1]?.[1] ?? 0)}、上限 ${fmtInt(limit)}`}
          onPointerMove={(e) => setHover(nearestIndex(points, plot.g, e.clientX - e.currentTarget.getBoundingClientRect().left))}
          onPointerLeave={() => setHover(null)}
        >
          <Axes g={plot.g} width={width} limit={limit} />
          <Series points={points} g={plot.g} />
          {hovered && (
            <g>
              <line class="crosshair" x1={plot.g.x(hovered[0])} x2={plot.g.x(hovered[0])} y1={MARGIN.top} y2={plot.g.y(0)} />
              <circle class="series-dot" cx={plot.g.x(hovered[0])} cy={plot.g.y(hovered[1])} r={DOT_RADIUS + 1} />
            </g>
          )}
        </svg>
      )}
      {plot && hover !== null && <Tooltip {...plot} index={hover} />}
    </div>
  );
};
