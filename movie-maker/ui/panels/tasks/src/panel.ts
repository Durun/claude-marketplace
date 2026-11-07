import { h } from "preact";
import { definePanel } from "@mm/sdk";
import { TasksPanel } from "./TasksPanel.tsx";

/** Tasks の項目は Shell が `commands` を持つパネルへ振り分け、無ければ `invoke` に渡す。 */
export const panel = definePanel({
  id: "tasks",
  label: "Tasks",
  column: "left",
  order: 10,
  render: ({ runCommand }) => h(TasksPanel, { onInvoke: runCommand }),
});
