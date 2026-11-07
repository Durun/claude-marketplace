import { mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";
import { generateCss } from "../theme/src/index.ts";

const DIST = fileURLToPath(new URL("../dist/", import.meta.url));
const ENTRY = fileURLToPath(new URL("../shell/src/main.tsx", import.meta.url));
const DEMO_ENTRY = fileURLToPath(new URL("../panels/inspector/demo/main.tsx", import.meta.url));
const MEDIA_DEMO_ENTRY = fileURLToPath(new URL("../panels/media/demo/main.tsx", import.meta.url));
const DELIVER_DEMO_ENTRY = fileURLToPath(new URL("../panels/deliver/demo/main.tsx", import.meta.url));
const STORY_DEMO_ENTRY = fileURLToPath(new URL("../panels/story/demo/main.tsx", import.meta.url));
const KEYFRAMES_DEMO_ENTRY = fileURLToPath(new URL("../panels/inspector/demo/keyframes.tsx", import.meta.url));
const REVIEW_DEMO_ENTRY = fileURLToPath(new URL("../panels/review/demo/main.tsx", import.meta.url));

// Tauri は dist/ をそのまま読む。相対パスだけにして、どの場所から開いても動くようにする。
const page = (title: string, name: string): string => `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${title}</title>
<link rel="stylesheet" href="./theme.css">
<link rel="stylesheet" href="./${name}.css">
</head>
<body>
<div id="root"></div>
<script defer src="./${name}.js"></script>
</body>
</html>
`;

await mkdir(DIST, { recursive: true });
await build({
  entryPoints: { shell: ENTRY, "inspector-demo": DEMO_ENTRY, "media-demo": MEDIA_DEMO_ENTRY, "story-demo": STORY_DEMO_ENTRY, "deliver-demo": DELIVER_DEMO_ENTRY, "review-demo": REVIEW_DEMO_ENTRY, "keyframes-demo": KEYFRAMES_DEMO_ENTRY },
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
await writeFile(`${DIST}index.html`, page("Movie Maker", "shell"));
// Inspector を fixtures のデータで確かめるページ。アプリには載せない。
await writeFile(`${DIST}inspector-demo.html`, page("Inspector demo", "inspector-demo"));
// Media をモックの API で動かすページ。アプリには載せない。
await writeFile(`${DIST}media-demo.html`, page("Media demo", "media-demo"));
// Story をモックの API で動かすページ。アプリには載せない。
await writeFile(`${DIST}story-demo.html`, page("Story demo", "story-demo"));
// Inspector のキーフレームの列と Graph を手元のレイヤーで動かすページ。アプリには載せない。
await writeFile(`${DIST}keyframes-demo.html`, page("Keyframes demo", "keyframes-demo"));
// Deliver をモックの API で動かすページ。アプリには載せない。
await writeFile(`${DIST}deliver-demo.html`, page("Deliver demo", "deliver-demo"));
// Review をモックの API で動かすページ。アプリには載せない。
await writeFile(`${DIST}review-demo.html`, page("Review demo", "review-demo"));
