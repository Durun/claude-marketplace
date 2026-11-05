import { mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";
import { generateCss } from "../theme/src/index.ts";

const DIST = fileURLToPath(new URL("../dist/", import.meta.url));
const ENTRY = fileURLToPath(new URL("../shell/src/main.tsx", import.meta.url));

// Tauri は dist/ をそのまま読む。相対パスだけにして、どの場所から開いても動くようにする。
const INDEX_HTML = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Movie Maker</title>
<link rel="stylesheet" href="./theme.css">
<link rel="stylesheet" href="./shell.css">
</head>
<body>
<div id="root"></div>
<script defer src="./shell.js"></script>
</body>
</html>
`;

await mkdir(DIST, { recursive: true });
await build({
  entryPoints: { shell: ENTRY },
  outdir: DIST,
  bundle: true,
  format: "iife",
  target: "chrome120",
  jsx: "automatic",
  jsxImportSource: "preact",
  sourcemap: true,
  logLevel: "info",
});
await writeFile(`${DIST}theme.css`, generateCss());
await writeFile(`${DIST}index.html`, INDEX_HTML);
