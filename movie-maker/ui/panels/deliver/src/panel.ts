import { h } from "preact";
import { definePanel } from "@mm/sdk";
import { createIpcDeliverApi } from "./api.ts";
import { DeliverPanel } from "./DeliverPanel.tsx";

/** Tasks の書き出しの項目は Deliver のタブを開く。 */
export const panel = definePanel({
  id: "deliver",
  label: "Deliver",
  column: "left",
  order: 50,
  createApi: createIpcDeliverApi,
  commands: ["export.start", "export.choose_target"],
  render: ({ api, docChanges }) => h(DeliverPanel, { api, refreshKey: docChanges }),
});
