// dist/ を開いてパネルのスクリーンショットを撮る。playwright は repo の依存にせず、
// PLAYWRIGHT_DIR（playwright を入れた場所）から読む。
// 使い方: pnpm build && PLAYWRIGHT_DIR=/tmp/pw node tools/screenshot.mjs [tasks|inspector|media|story]
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

// Media は偽のアプリの窓口で場面を作る。左の列と同じ幅で撮る。
const MEDIA_SCENES = ["idle", "importing", "failed", "empty", "cache"];

const shootMedia = async (browser) => {
  const outDir = here("../panels/media/screenshots/");
  await mkdir(outDir, { recursive: true });
  for (const theme of ["light", "dark"]) {
    for (const scene of MEDIA_SCENES) {
      const page = await browser.newPage({ viewport: { width: 262, height: 640 }, deviceScaleFactor: 2 });
      await page.goto(`${dist("media-demo.html")}?theme=${theme}&state=${scene}`);
      await page.waitForSelector(".mm-media-list");
      if (scene === "idle") {
        await page.waitForSelector(".mm-media-row img");
        await page.locator(".mm-media-row").first().click();
        await page.waitForSelector(".mm-media-wave-svg");
        await page.mouse.move(0, 0);
      }
      await page.waitForTimeout(600);
      await page.screenshot({ path: `${outDir}media-${theme}-${scene}.png` });
      await page.close();
    }
  }
};

// Story は偽のアプリの窓口で場面を作る。左の列と同じ幅で撮る。
const dragWords = async (page, from, to) => {
  const a = await page.locator(`[data-i="${from}"]`).boundingBox();
  const b = await page.locator(`[data-i="${to}"]`).boundingBox();
  await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
  await page.mouse.down();
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 6 });
  await page.mouse.up();
};

const shootStory = async (browser) => {
  const outDir = here("../panels/story/screenshots/");
  await mkdir(outDir, { recursive: true });
  for (const theme of ["light", "dark"]) {
    const open = async (state) => {
      const page = await browser.newPage({ viewport: { width: 282, height: 760 }, deviceScaleFactor: 2 });
      await page.goto(`${dist("story-demo.html")}?theme=${theme}&state=${state}`);
      await page.waitForSelector(".mm-story-word");
      return page;
    };
    const shot = async (page, name) => {
      await page.mouse.move(0, 0);
      await page.screenshot({ path: `${outDir}story-${theme}-${name}.png` });
    };
    const plain = await open("plain");
    await shot(plain, "reading");
    await dragWords(plain, 11, 17);
    await shot(plain, "selection");
    await plain.close();
    const topics = await open("topics");
    await shot(topics, "topics");
    const picks = topics.locator('input[aria-label="Pick topic"]');
    for (const i of [3, 0, 1]) await picks.nth(i).check();
    await shot(topics, "arrange");
    await topics.fill('input[aria-label="Search transcript"]', "ます");
    await topics.press('input[aria-label="Search transcript"]', "Enter");
    await topics.waitForSelector(".mm-story-hit");
    await shot(topics, "search");
    await topics.close();
  }
};

const browser = await chromium.launch();
if (!which || which === "tasks") await shootTasks(browser);
if (!which || which === "inspector") await shootInspector(browser);
if (!which || which === "media") await shootMedia(browser);
if (!which || which === "story") await shootStory(browser);
await browser.close();
