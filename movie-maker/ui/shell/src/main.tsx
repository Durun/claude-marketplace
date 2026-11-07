import { render } from "preact";
import { createClient, createIpcTransport } from "@mm/sdk";
import { followTheme } from "./followTheme.ts";
import { createPanelApis } from "./registry.ts";
import { Shell } from "./Shell.tsx";
import "./shell.css";

const client = createClient(createIpcTransport());

const sendOp = async (op: Parameters<typeof client.sendOp>[0]): Promise<string | null> => {
  const result = await client.sendOp(op);
  return result.ok ? null : `${op.op}: ${result.error}`;
};

followTheme(document.documentElement, location.search, matchMedia.bind(window));
const root = document.getElementById("root");
if (root) render(<Shell sendOp={sendOp} apis={createPanelApis()} />, root);
