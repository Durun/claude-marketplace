import { render } from "preact";
import { invoke } from "@mm/sdk";
import { TasksPanel } from "@mm/tasks";
import "./shell.css";

const THEME_PARAM = "theme";

/** 指定が無ければ OS の配色に合わせる。?theme=dark で固定できる（画面の確認用）。 */
const pickTheme = (): "light" | "dark" => {
  const forced = new URLSearchParams(location.search).get(THEME_PARAM);
  if (forced === "light" || forced === "dark") return forced;
  return matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
};

/** 左の列。右の列のパネルが出来たらここと並べる。 */
const Shell = () => (
  <aside class="mm-shell-left">
    <TasksPanel onInvoke={(command) => void invoke(command)} />
  </aside>
);

document.documentElement.dataset.theme = pickTheme();
const root = document.getElementById("root");
if (root) render(<Shell />, root);
