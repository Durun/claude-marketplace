// dist/ を開いてパネルのスクリーンショットを撮る。playwright は repo の依存にせず、
// PLAYWRIGHT_DIR（playwright を入れた場所）から読む。
// 使い方: pnpm build && PLAYWRIGHT_DIR=/tmp/pw node tools/screenshot.mjs [tasks|inspector]
import { createRequire } from "node:module";
import { mkdir, readFile } from "node:fs/promises";
import { fileURLToPath, pathToFileURL } from "node:url";

const require = createRequire(`${process.env.PLAYWRIGHT_DIR ?? "."}/`);
const { chromium } = require("playwright");

const here = (rel) => fileURLToPath(new URL(rel, import.meta.url));
const dist = (name) => pathToFileURL(here(`../dist/${name}`)).href;
const which = process.argv[2];

const shootTasks = async (browser) => {
  const outDir = here("../panels/tasks/screenshots/");
  await mkdir(outDir, { recursive: true });
  for (const theme of ["light", "dark"]) {
    const page = await browser.newPage({ viewport: { width: 260, height: 460 }, deviceScaleFactor: 2 });
    await page.goto(`${dist("index.html")}?theme=${theme}`);
    await page.screenshot({ path: `${outDir}tasks-${theme}-open.png` });
    const toggles = page.locator(".mm-tasks-toggle");
    for (const i of [1, 2, 3]) await toggles.nth(i).click();
    await page.mouse.move(0, 0);
    await page.screenshot({ path: `${outDir}tasks-${theme}-collapsed.png` });
    await page.close();
  }
};

const shootInspector = async (browser) => {
  const outDir = here("../panels/inspector/screenshots/");
  await mkdir(outDir, { recursive: true });
  for (const theme of ["light", "dark"]) {
    const page = await browser.newPage({ viewport: { width: 760, height: 900 }, deviceScaleFactor: 2 });
    await page.goto(`${dist("inspector-demo.html")}?theme=${theme}`);
    await page.screenshot({ path: `${outDir}inspector-${theme}-title.png` });
    await page.selectOption("select[aria-label=layer]", "l-footage");
    await page.screenshot({ path: `${outDir}inspector-${theme}-footage.png` });
    await page.close();
  }
  // 本体の画面: 右の列に載った Inspector。
  const project = JSON.parse(await readFile(here("../../fixtures/ui/inspector.json"), "utf8"));
  const detail = { layer: project.compositions[0].layers[0], assets: project.assets };
  const page = await browser.newPage({ viewport: { width: 900, height: 900 }, deviceScaleFactor: 2 });
  await page.goto(`${dist("index.html")}?theme=light`);
  await page.waitForSelector(".mm-insp");
  await page.waitForTimeout(200);
  await page.evaluate((d) => window.dispatchEvent(new CustomEvent("mm:layer-selected", { detail: d })), detail);
  await page.waitForSelector(".mm-insp-header");
  await page.screenshot({ path: `${outDir}shell-light.png` });
  await page.close();
};

const browser = await chromium.launch();
if (!which || which === "tasks") await shootTasks(browser);
if (!which || which === "inspector") await shootInspector(browser);
await browser.close();
