import { h } from "preact";
import { definePanel } from "@mm/sdk";
import { createIpcReviewApi } from "./api.ts";
import { ReviewPanel } from "./ReviewPanel.tsx";

export const panel = definePanel({
  id: "review",
  label: "Review",
  column: "right",
  order: 20,
  createApi: createIpcReviewApi,
  render: ({ api, docChanges, active }) => h(ReviewPanel, { api, refreshKey: docChanges, active }),
});
