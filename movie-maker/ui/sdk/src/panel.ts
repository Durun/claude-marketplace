import type { ComponentChild } from "preact";
import type { Asset, Layer, Op } from "./generated/ops.ts";

/** パネルを出す列。列の中のタブは `order` の小さい順に並ぶ。 */
export type PanelColumn = "left" | "right";

/** Timeline で選ばれているレイヤーと、それが指す素材。 */
export interface LayerSelection {
  layer: Layer | null;
  assets?: Asset[];
}

/** Shell がパネルの `render` に渡すもの。 */
export interface PanelContext<A> {
  api: A;
  /** `mm:doc-changed` が届いた回数。読み直しの合図に使う。 */
  docChanges: number;
  /** タブを選んだ回数。開くたびに読み直したいパネルが使う。 */
  visits: number;
  /** Tasks の項目で `commands` のどれかが選ばれた回数。 */
  requests: number;
  /** このパネルのタブが選ばれているか。 */
  active: boolean;
  selection: LayerSelection;
  /** 操作を送る。拒否の理由は Shell が画面に出す。 */
  sendOp: (op: Op) => Promise<void>;
  /** Tasks の項目を実行する。担当のパネルがあればそのタブを開く。 */
  runCommand: (command: string) => void;
}

/**
 * パネルの登録 1 件。パネルのパッケージが `src/panel.ts` で作り、`ui/shell/src/panels.ts` が 1 行で集める。
 * `createApi` の無いパネルは常に出し、有るパネルは Shell に API が渡されたときだけ出す。
 */
export interface PanelDescriptor<A = undefined> {
  id: string;
  label: string;
  column: PanelColumn;
  order: number;
  createApi?: () => A;
  /** Tasks の項目のうち、このパネルを開いて `requests` を増やすもの。 */
  commands?: readonly string[];
  render: (ctx: PanelContext<A>) => ComponentChild;
}

/** API の型を消した記述子。Shell が種類の違うパネルを 1 つの配列で持つ。 */
export interface AnyPanel {
  id: string;
  label: string;
  column: PanelColumn;
  order: number;
  createApi?: () => unknown;
  commands?: readonly string[];
  render: (ctx: PanelContext<unknown>) => ComponentChild;
}

/** 記述子を作る。`render` の API の型は `createApi` の返り値から決まる。 */
export const definePanel = <A = undefined>(panel: PanelDescriptor<A>): AnyPanel => ({
  ...panel,
  render: (ctx) => panel.render(ctx as PanelContext<A>),
});
