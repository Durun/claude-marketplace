// 実際のアプリ（WebView2 を --remote-debugging-port=9333 で起動したもの）に CDP でつなぎ、素材を 1 本取り込んで
// Media の画面を撮る。使い方: PLAYWRIGHT_DIR=/tmp/pw node tools/app-media.mjs <素材のパス> <出力の PNG>
import { createRequire } from "node:module";

const require = createRequire(`${process.env.PLAYWRIGHT_DIR ?? "."}/`);
const { chromium } = require("playwright");

const [clip, out] = process.argv.slice(2);
const browser = await chromium.connectOverCDP("http://127.0.0.1:9333");
const page = browser.contexts()[0].pages()[0];
await page.setViewportSize({ width: 800, height: 760 });
await page.click('.mm-shell-tab:has-text("Media")');
await page.waitForSelector(".mm-media-list");
console.log("取り込む前の行:", await page.locator(".mm-media-row").count());
await page.evaluate((p) => window.__TAURI__.core.invoke("media_import", { paths: [p] }), clip);
await page.waitForFunction(() => document.querySelectorAll(".mm-media-row img").length > 0);
await page.locator(".mm-media-row").last().click();
await page.waitForSelector(".mm-media-wave-svg");
await page.mouse.move(700, 700);
await page.waitForTimeout(500);
console.log("取り込んだ後の行:", await page.locator(".mm-media-row").count());
await page.screenshot({ path: out });
await browser.close();
