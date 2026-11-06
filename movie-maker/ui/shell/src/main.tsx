import { render } from "preact";
import { invoke } from "@mm/sdk";
import { TasksPanel } from "@mm/tasks";
import { followTheme } from "./followTheme.ts";
import "./shell.css";

/** 左の列。右の列のパネルが出来たらここと並べる。 */
const Shell = () => (
  <aside class="mm-shell-left">
    <TasksPanel onInvoke={(command) => void invoke(command)} />
  </aside>
);

followTheme(document.documentElement, location.search, matchMedia.bind(window));
const root = document.getElementById("root");
if (root) render(<Shell />, root);
