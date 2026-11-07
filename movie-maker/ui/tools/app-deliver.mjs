// 実際のアプリ（WebView2 を --remote-debugging-port=9333 で起動したもの）に CDP でつなぎ、Deliver でレンダーターゲットを
// 2 つ足し、キューに積んで書き出し、終わるまでを撮る。素材と project.mmdb は用意してある前提。
// 使い方: PLAYWRIGHT_DIR=/tmp/pw node tools/app-deliver.mjs <出力のフォルダー>
import { createRequire } from "node:module";
import { mkdir } from "node:fs/promises";

const require = createRequire(`${process.env.PLAYWRIGHT_DIR ?? "."}/`);
const { chromium } = require("playwright");

const [outDir] = process.argv.slice(2);
await mkdir(outDir, { recursive: true });
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${process.env.CDP_PORT ?? 9333}`);
const page = browser.contexts()[0].pages()[0];
await page.setViewportSize({ width: 800, height: 900 });
const shot = async (name) => {
  await page.mouse.move(0, 0);
  await page.screenshot({ path: `${outDir}/${name}.png` });
};

await page.click('.mm-shell-tab:has-text("Deliver")');
await page.waitForSelector(".mm-deliver");

// プリセットから足す。2 本目は縦の Shorts。
const addPreset = async (label) => {
  await page.selectOption('select[aria-label="Preset"]', { label });
  await page.getByRole("button", { name: "Add target" }).click();
  await page.waitForSelector(".mm-deliver-form");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await page.waitForSelector(".mm-deliver-form", { state: "detached" });
};
await addPreset("YouTube 1080p");
await addPreset("YouTube Shorts 1080x1920");
await page.waitForSelector(".mm-deliver-row >> nth=1");
await shot("1-targets");

await page.getByLabel("Queue youtube-1080p").check();
await page.getByLabel("Queue youtube-shorts").check();
await shot("2-queued");
await page.getByRole("button", { name: "Export 2 targets" }).click();

// 1 本目の途中。
await page.waitForFunction(() => {
  const bar = document.querySelector("[role=progressbar]");
  const now = Number(bar?.getAttribute("aria-valuenow") ?? 0);
  return now > 0 && now < 100;
});
await shot("3-running");

await page.waitForSelector(".mm-deliver-queue h3:has-text('Export finished')", { timeout: 300000 });
await shot("4-finished");
const items = await page.evaluate(() => window.__TAURI__.core.invoke("export_status"));
process.stdout.write(`${JSON.stringify(items)}\n`);
await browser.close();
