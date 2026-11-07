import { mkdir, readdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";
import { generateCss } from "../theme/src/index.ts";

const DIST = fileURLToPath(new URL("../dist/", import.meta.url));
const ENTRY = fileURLToPath(new URL("../shell/src/main.tsx", import.meta.url));
const PANELS = fileURLToPath(new URL("../panels/", import.meta.url));

interface Demo {
  /** dist/ に出す名前。`panels/<panel>/demo/main.tsx` は `<panel>-demo`、`<name>.tsx` は `<name>-demo`。 */
  name: string;
  title: string;
  entry: string;
}

const capitalize = (s: string): string => s.charAt(0).toUpperCase() + s.slice(1);

/** `panels/<panel>/demo/*.tsx` を走査して、パネルごとのデモのページを集める。アプリには載せない。 */
const findDemos = async (): Promise<Demo[]> => {
  const demos: Demo[] = [];
  for (const panel of (await readdir(PANELS)).sort()) {
    const dir = `${PANELS}${panel}/demo/`;
    const files = await readdir(dir).catch(() => [] as string[]);
    for (const file of files.filter((f) => f.endsWith(".tsx")).sort()) {
      const base = file === "main.tsx" ? panel : file.slice(0, -".tsx".length);
      demos.push({ name: `${base}-demo`, title: `${capitalize(base)} demo`, entry: `${dir}${file}` });
    }
  }
  return demos;
};

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

const demos = await findDemos();
await mkdir(DIST, { recursive: true });
await build({
  entryPoints: { shell: ENTRY, ...Object.fromEntries(demos.map((d) => [d.name, d.entry])) },
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
for (const d of demos) await writeFile(`${DIST}${d.name}.html`, page(d.title, d.name));
