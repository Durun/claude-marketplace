import type { Keyframe, Op, OpName, OpOf, ParamRef, ParamValue, RationalTime } from "@mm/sdk";
import type { ProposalOp, ProposalScope } from "./api.ts";

const MS_PER_SECOND = 1000;
const SECONDS_PER_MINUTE = 60;
const MINUTES_PER_HOUR = 60;
const HOURS_PER_DAY = 24;
const JUST_NOW_SECONDS = 45;
const SECOND_DIGITS = 2;
const CENTI = 100;
const PAD_WIDTH = 2;
const MINUS = "−";

/** 有理数の時刻を秒にする。分母が 0 のときは 0。 */
export const rationalSeconds = (t: RationalTime): number => (t.den === 0 ? 0 : t.num / t.den);

/** 秒を符号つきで `+0.20s` / `−0.40s` にする。符号の `−` は U+2212。 */
export const signedSeconds = (seconds: number): string => {
  const text = `${Math.abs(seconds).toFixed(SECOND_DIGITS)}s`;
  return seconds < 0 ? `${MINUS}${text}` : `+${text}`;
};

const plainSeconds = (t: RationalTime): string => `${rationalSeconds(t).toFixed(SECOND_DIGITS)}s`;

/** 秒を `m:ss.cc` にする（シーケンスの範囲の表示）。 */
export const clock = (seconds: number): string => {
  const centis = Math.round(Math.max(0, seconds) * CENTI);
  const m = Math.floor(centis / (SECONDS_PER_MINUTE * CENTI));
  const s = Math.floor(centis / CENTI) % SECONDS_PER_MINUTE;
  const c = centis % CENTI;
  return `${m}:${String(s).padStart(PAD_WIDTH, "0")}.${String(c).padStart(PAD_WIDTH, "0")}`;
};

/** 提案が出た時刻を、いまからの相対で書く。 */
export const relativeTime = (atMs: number, nowMs: number): string => {
  const seconds = Math.max(0, Math.round((nowMs - atMs) / MS_PER_SECOND));
  if (seconds < JUST_NOW_SECONDS) return "just now";
  const minutes = Math.round(seconds / SECONDS_PER_MINUTE);
  if (minutes < MINUTES_PER_HOUR) return `${minutes} min ago`;
  const hours = Math.round(minutes / MINUTES_PER_HOUR);
  if (hours < HOURS_PER_DAY) return `${hours} h ago`;
  return `${Math.round(hours / HOURS_PER_DAY)} d ago`;
};

const plural = (n: number, noun: string): string => `${n} ${noun}${n === 1 ? "" : "s"}`;

const valueText = (v: ParamValue): string => {
  switch (v.type) {
    case "number":
    case "bool":
      return String(v.value);
    case "vec2":
    case "vec3":
      return `(${v.value.join(", ")})`;
    default:
      return v.type;
  }
};

const paramOwner = (target: ParamRef): string => {
  if (target.layer) return `layer ${target.layer}`;
  if (target.cut) return `cut ${target.cut}`;
  return "project";
};

const keyframeText = (k: Keyframe): string => `${plainSeconds(k.time)} to ${valueText(k.value)}`;

type Describers = { [N in OpName]?: (op: OpOf<N>) => string };

const DESCRIBERS: Describers = {
  ripple: (o) => `Ripple cut ${o.cut} ${o.edge} edge by ${signedSeconds(rationalSeconds(o.by))}`,
  roll: (o) => `Roll cut ${o.cut} by ${signedSeconds(rationalSeconds(o.by))}`,
  slip: (o) => `Slip cut ${o.cut} by ${signedSeconds(rationalSeconds(o.by))}`,
  slide: (o) => `Slide cut ${o.cut} by ${signedSeconds(rationalSeconds(o.by))}`,
  split: (o) => `Split cut ${o.cut} at ${plainSeconds(o.at)} (new cut ${o.new_cut})`,
  join: (o) => `Join cut ${o.cut} with ${o.next}`,
  insert: (o) => `Insert cut ${o.cut.id} into sequence ${o.sequence} at ${o.index}`,
  overwrite: (o) => `Replace cut ${o.cut} with ${o.replacement.id}`,
  extract: (o) => `Extract cut ${o.cut}`,
  lift: (o) => `Lift cut ${o.cut}`,
  disable: (o) => `${o.disabled === false ? "Enable" : "Disable"} cut ${o.cut}`,
  insertCuts: (o) => `Insert ${plural(o.cuts.length, "cut")} into sequence ${o.sequence} at ${o.index}`,
  extractCuts: (o) => `Extract ${plural(o.count, "cut")} from sequence ${o.sequence} at ${o.index}`,
  trimLayer: (o) => `Trim layer ${o.layer} ${o.edge} edge by ${signedSeconds(rationalSeconds(o.by))}`,
  addLayer: (o) => `Add layer ${o.layer.id} to ${o.composition}`,
  removeLayer: (o) => `Remove layer ${o.layer}`,
  moveLayer: (o) => `Move layer ${o.layer} to position ${o.index}`,
  setParam: (o) => `Set ${o.target.path} on ${paramOwner(o.target)} to ${valueText(o.value)}`,
  removeParam: (o) => `Remove ${o.target.path} on ${paramOwner(o.target)}`,
  setKeyframe: (o) => `Set keyframe on ${o.target.path} of ${paramOwner(o.target)} at ${keyframeText(o.keyframe)}`,
  removeKeyframe: (o) => `Remove keyframe on ${o.target.path} of ${paramOwner(o.target)} at ${plainSeconds(o.time)}`,
  addMarker: (o) => `Add marker "${o.marker.name}" at ${plainSeconds(o.marker.time)}`,
  removeMarker: (o) => `Remove marker ${o.marker}`,
};

/** 操作を英語の 1 行にする。知らない操作は `opName (id...)`。 */
export const describeOp = ({ op, touches }: ProposalOp): string => {
  const describe = DESCRIBERS[op.op] as ((o: Op) => string) | undefined;
  if (describe) return describe(op);
  return touches.length > 0 ? `${op.op} (${touches.join(", ")})` : op.op;
};

/** 範囲を `Sequence s1 · 0:01.50–0:04.00 · 2 cuts` などの行にする。 */
export const describeScope = (scope: ProposalScope): string[] => [
  ...scope.sequences.map(
    (s) => `Sequence ${s.id} · ${clock(s.start)}–${clock(s.end)} · ${plural(s.cuts.length, "cut")}`,
  ),
  ...scope.layers.map((l) => `Layer ${l.id} (${l.comp})`),
  ...(scope.other.length > 0 ? [`Other: ${scope.other.join(", ")}`] : []),
];

/** 承認ボタンの文字。全部のときは `Approve`、一部のときは `Approve 2 of 3`。 */
export const approveLabel = (selected: number, total: number): string =>
  selected === total ? "Approve" : `Approve ${selected} of ${total}`;
