import { h } from "preact";
import { definePanel } from "@mm/sdk";
import { createIpcMediaApi } from "./api.ts";
import { MediaPanel } from "./MediaPanel.tsx";

/** Tasks の取り込みの項目（`media.import`）は Media を開いてファイルの選択を始める。 */
export const panel = definePanel({
  id: "media",
  label: "Media",
  column: "left",
  order: 20,
  createApi: createIpcMediaApi,
  commands: ["media.import"],
  render: ({ api, requests, docChanges }) => h(MediaPanel, { api, importRequest: requests, refreshKey: docChanges }),
});
