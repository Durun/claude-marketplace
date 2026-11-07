import { h } from "preact";
import { definePanel } from "@mm/sdk";
import { createIpcTransitionApi } from "./api.ts";
import { TransitionPanel } from "./TransitionPanel.tsx";

/** タブを開くたびに読み直すため、`visits` も読み直しの合図に足す。 */
export const panel = definePanel({
  id: "transition",
  label: "Transition",
  column: "left",
  order: 35,
  createApi: createIpcTransitionApi,
  render: ({ api, visits, docChanges }) => h(TransitionPanel, { api, refreshKey: visits + docChanges }),
});
