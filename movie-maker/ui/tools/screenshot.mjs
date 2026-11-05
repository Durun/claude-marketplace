// dist/ を開いて tasks パネルの絵を撮る。playwright は repo の依存にせず、
// PLAYWRIGHT_DIR（playwright を入れた場所）から読む。使い方: pnpm build && PLAYWRIGHT_DIR=/tmp/pw node tools/screenshot.mjs
import { createRequire } from "node:module";
import { mkdir } from "node:fs/promises";
import { fileURLToPath, pathToFileURL } from "node:url";

const require = createRequire(`${process.env.PLAYWRIGHT_DIR ?? "."}/`);
const { chromium } = require("playwright");

const dist = fileURLToPath(new URL("../dist/index.html", import.meta.url));
const outDir = fileURLToPath(new URL("../panels/tasks/screenshots/", import.meta.url));
await mkdir(outDir, { recursive: true });

const browser = await chromium.launch();
for (const theme of ["light", "dark"]) {
  const page = await browser.newPage({ viewport: { width: 260, height: 460 }, deviceScaleFactor: 2 });
  await page.goto(`${pathToFileURL(dist).href}?theme=${theme}`);
  await page.screenshot({ path: `${outDir}tasks-${theme}-open.png` });
  const toggles = page.locator(".mm-tasks-toggle");
  for (const i of [1, 2, 3]) await toggles.nth(i).click();
  await page.mouse.move(0, 0);
  await page.screenshot({ path: `${outDir}tasks-${theme}-collapsed.png` });
  await page.close();
}
await browser.close();
