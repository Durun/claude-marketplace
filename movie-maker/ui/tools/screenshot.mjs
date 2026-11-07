// dist/ を開いてパネルのスクリーンショットを撮る。撮り方は各パネルの `panels/<panel>/screenshots.mjs`
// （default export の `(browser, { dist }) => Promise<void>`）にあり、ここは走査して名前で選ぶだけ。
// playwright は repo の依存にせず、PLAYWRIGHT_DIR（playwright を入れた場所）から読む。
// 使い方: pnpm build && PLAYWRIGHT_DIR=/tmp/pw node tools/screenshot.mjs [<パネルのディレクトリ名>]
import { createRequire } from "node:module";
import { readdir } from "node:fs/promises";
import { fileURLToPath, pathToFileURL } from "node:url";

const require = createRequire(`${process.env.PLAYWRIGHT_DIR ?? "."}/`);
const { chromium } = require("playwright");

const PANELS = new URL("../panels/", import.meta.url);
const dist = (name) => pathToFileURL(fileURLToPath(new URL(`../dist/${name}`, import.meta.url))).href;
const which = process.argv[2];

const names = (await readdir(PANELS)).sort();
const shooters = [];
for (const name of names) {
  if (which && which !== name) continue;
  const url = new URL(`${name}/screenshots.mjs`, PANELS);
  const mod = await import(url.href).catch((e) => (e.code === "ERR_MODULE_NOT_FOUND" ? null : Promise.reject(e)));
  if (mod) shooters.push(mod.default);
}
if (which && shooters.length === 0) throw new Error(`screenshots.mjs のあるパネルが無い: ${which}`);

const browser = await chromium.launch();
try {
  for (const shoot of shooters) await shoot(browser, { dist });
} finally {
  await browser.close();
}
