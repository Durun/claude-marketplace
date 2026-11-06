import { render } from "preact";
import { createClient, createIpcMediaApi, createIpcStoryApi, createIpcTransport } from "@mm/sdk";
import { followTheme } from "./followTheme.ts";
import { Shell } from "./Shell.tsx";
import "./shell.css";

const client = createClient(createIpcTransport());

const sendOp = (op: Parameters<typeof client.sendOp>[0]): void => {
  void client.sendOp(op).then((result) => {
    if (!result.ok) console.warn("[mm-shell] op failed", op.op, result.error);
  });
};

followTheme(document.documentElement, location.search, matchMedia.bind(window));
const root = document.getElementById("root");
if (root) render(<Shell sendOp={sendOp} media={createIpcMediaApi()} story={createIpcStoryApi()} />, root);
