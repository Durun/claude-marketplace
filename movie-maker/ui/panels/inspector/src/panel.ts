import { h } from "preact";
import { definePanel } from "@mm/sdk";
import { KeyframePanels } from "./KeyframePanels.tsx";

/** 右の列の先頭。Inspector とその下の Graph を 1 つのタブに載せる。 */
export const panel = definePanel({
  id: "inspector",
  label: "Inspector",
  column: "right",
  order: 10,
  render: ({ selection, sendOp }) =>
    h(KeyframePanels, { layer: selection.layer, assets: selection.assets, onOp: sendOp }),
});
