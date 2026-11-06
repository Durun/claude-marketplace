import type { Asset, Layer, Op, ParamValue } from "@mm/sdk";
import { Editor } from "./editors.tsx";
import { fieldSpecFor } from "./fields.ts";
import {
  DEFAULT_HINTS,
  describeLayer,
  hasKeyframes,
  setParamOp,
  shownValue,
  type FieldRow,
  type ParamHints,
} from "./model.ts";

export interface InspectorPanelProps {
  /** 選んだレイヤー。無ければ空の表示。 */
  layer: Layer | null;
  /** `asset_ref` の選択肢。 */
  assets?: readonly Asset[];
  hints?: ParamHints;
  /** 値を変えたときに操作を渡す。送るのは呼び出し側（sdk の client）。 */
  onOp: (op: Op) => void;
}

interface RowViewProps {
  row: FieldRow;
  assets: readonly Asset[];
  onCommit: (path: string, value: ParamValue) => void;
}

const KeyframeMark = ({ count }: { count: number }) => (
  <span
    class="mm-insp-kf"
    role="img"
    title={`${count} keyframes`}
    aria-label={`${count} keyframes`}
  />
);

const RowView = ({ row, assets, onCommit }: RowViewProps) => {
  const animated = hasKeyframes(row.param);
  const value = shownValue(row.param);
  return (
    <div class="mm-insp-row" data-animated={animated}>
      <label class="mm-insp-label">
        {animated && <KeyframeMark count={row.param.keyframes?.length ?? 0} />}
        <span>{row.label}</span>
      </label>
      <div class="mm-insp-value">
        <Editor
          spec={fieldSpecFor(value.type)}
          value={value}
          hint={row.hint}
          assets={assets}
          disabled={animated}
          label={row.label}
          onCommit={(next) => onCommit(row.path, next)}
        />
      </div>
    </div>
  );
};

/** 選んだレイヤーのパラメータを、値の型の JSON Schema から作った入力欄で編集する。 */
export const InspectorPanel = ({ layer, assets = [], hints = DEFAULT_HINTS, onOp }: InspectorPanelProps) => {
  if (!layer) {
    return <div class="mm-insp mm-insp-empty">No layer selected</div>;
  }
  return (
    <div class="mm-insp">
      <h2 class="mm-insp-header">{layer.id}</h2>
      {describeLayer(layer, hints).map((section) => (
        <section key={section.id} class="mm-insp-section">
          <h3 class="mm-insp-title">{section.title}</h3>
          <div class="mm-insp-body">
            {section.rows.map((row) => (
              <RowView
                key={row.path}
                row={row}
                assets={assets}
                onCommit={(path, value) => onOp(setParamOp(layer.id, path, value))}
              />
            ))}
          </div>
        </section>
      ))}
    </div>
  );
};
