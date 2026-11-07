import { h } from "preact";
import { definePanel } from "@mm/sdk";
import { createIpcMixerApi } from "./api.ts";
import { MixerPanel } from "./MixerPanel.tsx";

export const panel = definePanel({
  id: "mixer",
  label: "Mixer",
  column: "left",
  order: 40,
  createApi: createIpcMixerApi,
  render: ({ api, docChanges }) => h(MixerPanel, { api, refreshKey: docChanges }),
});
