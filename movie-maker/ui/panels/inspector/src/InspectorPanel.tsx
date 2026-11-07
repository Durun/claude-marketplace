import { type Asset, type Layer, type Op, type ParamRef, type ParamValue, evalParam } from "@mm/sdk";
import { Editor } from "./editors.tsx";
import { AddMaskButton, FadeRows, RemoveMaskButton } from "./Extras.tsx";
import { fieldSpecFor } from "./fields.ts";
import { KeyframeNav } from "./KeyframeNav.tsx";
import { valueAtOp, type Playhead } from "./keyframes.ts";
import {
  DEFAULT_HINTS,
  describeLayer,
  hasKeyframes,
  setParamOp,
  shownValue,
  type FieldRow,
  type ParamHints,
} from "./model.ts";

/** fps が渡されないときのフレームの数え方。 */
const DEFAULT_FPS = 30;

export interface InspectorPanelProps {
  /** 選んだレイヤー。無ければ空の表示。 */
  layer: Layer | null;
  /** `asset_ref` の選択肢。 */
  assets?: readonly Asset[];
  hints?: ParamHints;
  /** 値を変えたときに操作を渡す。送るのは呼び出し側（sdk の client）。 */
  onOp: (op: Op) => void;
  /** 再生位置（コンポジションの秒）。渡すとキーフレームナビゲーターを出し、動きのある値を再生位置で編集する。 */
  time?: number;
  /** コンポジションの fps。キーフレームの時刻をフレームにそろえる。 */
  fps?: number;
  /** 前後のキーフレームへ再生位置を動かす。 */
  onSeek?: (sec: number) => void;
  /** Graph に出しているパラメータのパス。 */
  selectedPath?: string | null;
  /** 行の名前を押したときに、そのパラメータを Graph に出す。 */
  onSelectParam?: (path: string) => void;
}

interface RowViewProps {
  row: FieldRow;
  target: ParamRef;
  assets: readonly Asset[];
  at: Playhead | null;
  selected: boolean;
  onOp: (op: Op) => void;
  onSeek?: (sec: number) => void;
  onSelect?: () => void;
}

const KeyframeMark = ({ count }: { count: number }) => (
  <span
    class="mm-insp-kf"
    role="img"
    title={`${count} keyframes`}
    aria-label={`${count} keyframes`}
  />
);

const RowLabel = ({ row, onSelect }: { row: FieldRow; onSelect?: () => void }) => {
  const count = row.param.keyframes?.length ?? 0;
  const body = (
    <>
      {count > 0 && <KeyframeMark count={count} />}
      <span>{row.label}</span>
    </>
  );
  if (!onSelect) return <label class="mm-insp-label">{body}</label>;
  return (
    <button type="button" class="mm-insp-label mm-insp-pick" title="Show in Graph" onClick={onSelect}>
      {body}
    </button>
  );
};

const RowView = ({ row, target, assets, at, selected, onOp, onSeek, onSelect }: RowViewProps) => {
  const animated = hasKeyframes(row.param);
  const value: ParamValue = at && animated ? evalParam(row.param, at.sec) : shownValue(row.param);
  const commit = (next: ParamValue) =>
    onOp(animated && at ? valueAtOp(target, row.param, at, next) : setParamOp(target.layer ?? "", row.path, next));
  return (
    <div class="mm-insp-row" data-animated={animated} data-selected={selected}>
      <RowLabel row={row} onSelect={onSelect} />
      <div class="mm-insp-value">
        <Editor
          spec={fieldSpecFor(value.type)}
          value={value}
          hint={row.hint}
          assets={assets}
          disabled={animated && !at}
          label={row.label}
          onCommit={commit}
        />
        {at && <KeyframeNav target={target} param={row.param} at={at} label={row.label} onOp={onOp} onSeek={onSeek} />}
      </div>
    </div>
  );
};

/**
 * 選んだレイヤーのパラメータを、値の型の JSON Schema から作った入力欄で編集する。
 * 再生位置を渡すと、各パラメータにキーフレームナビゲーター（前へ・足す外す・次へ・補間）を出す。
 */
export const InspectorPanel = (props: InspectorPanelProps) => {
  const { layer, assets = [], hints = DEFAULT_HINTS, onOp, time, onSeek, selectedPath, onSelectParam } = props;
  if (!layer) {
    return <div class="mm-insp mm-insp-empty">No layer selected</div>;
  }
  const at: Playhead | null = time === undefined ? null : { sec: time, fps: props.fps ?? DEFAULT_FPS };
  return (
    <div class="mm-insp">
      <h2 class="mm-insp-header">{layer.id}</h2>
      {describeLayer(layer, hints).map((section) => (
        <section key={section.id} class="mm-insp-section">
          <h3 class="mm-insp-title">
            {section.title}
            {section.maskId !== undefined && <RemoveMaskButton maskId={section.maskId} onOp={onOp} />}
          </h3>
          <div class="mm-insp-body">
            {section.rows.map((row) => (
              <RowView
                key={row.path}
                row={row}
                target={{ layer: layer.id, path: row.path }}
                assets={assets}
                at={at}
                selected={selectedPath === row.path}
                onOp={onOp}
                onSeek={onSeek}
                onSelect={onSelectParam && (() => onSelectParam(row.path))}
              />
            ))}
            {section.id === "audio" && <FadeRows layer={layer} onOp={onOp} />}
          </div>
        </section>
      ))}
      <section class="mm-insp-section">
        <h3 class="mm-insp-title">Masks</h3>
        <div class="mm-insp-body">
          <AddMaskButton layer={layer} onOp={onOp} />
        </div>
      </section>
    </div>
  );
};
