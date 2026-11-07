import { h } from "preact";
import { definePanel } from "@mm/sdk";
import { createIpcStoryApi } from "./api.ts";
import { StoryPanel } from "./StoryPanel.tsx";

/** タブを開くたびに読み直すため、`visits` も読み直しの合図に足す。 */
export const panel = definePanel({
  id: "story",
  label: "Story",
  column: "left",
  order: 30,
  createApi: createIpcStoryApi,
  render: ({ api, visits, docChanges }) => h(StoryPanel, { api, refreshKey: visits + docChanges }),
});
