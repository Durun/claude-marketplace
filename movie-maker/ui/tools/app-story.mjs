// 実際のアプリ（WebView2 を --remote-debugging-port=9333 で起動したもの）に CDP でつなぎ、素材を取り込んで
// Story で話題を作り、並べて再生するまでを撮る。文字起こしは tools/transcribe で cache/ingest/ に作ってある前提。
// 使い方: PLAYWRIGHT_DIR=/tmp/pw node tools/app-story.mjs <素材のパス> <出力のフォルダー>
import { createRequire } from "node:module";
import { mkdir } from "node:fs/promises";

const require = createRequire(`${process.env.PLAYWRIGHT_DIR ?? "."}/`);
const { chromium } = require("playwright");

const [clip, outDir] = process.argv.slice(2);
await mkdir(outDir, { recursive: true });
const browser = await chromium.connectOverCDP("http://127.0.0.1:9333");
const page = browser.contexts()[0].pages()[0];
await page.setViewportSize({ width: 1100, height: 900 });
const invoke = (command, args) => page.evaluate(([c, a]) => window.__TAURI__.core.invoke(c, a), [command, args ?? {}]);
const shot = async (name) => {
  await page.mouse.move(1000, 800);
  await page.screenshot({ path: `${outDir}/${name}.png` });
};
const wait = (ms) => page.waitForTimeout(ms);

// 単語の行ごとの矩形。折り返された単語は複数の矩形に分かれる。
const rectOf = (i, last) =>
  page.evaluate(
    ([index, tail]) => {
      const rects = [...document.querySelector(`[data-i="${index}"]`).getClientRects()];
      const r = tail ? rects.at(-1) : rects[0];
      return { x: r.x, y: r.y, w: r.width, h: r.height };
    },
    [i, last],
  );

const drag = async (from, to) => {
  await page.locator(`[data-i="${from}"]`).scrollIntoViewIfNeeded();
  await page.locator(`[data-i="${to}"]`).scrollIntoViewIfNeeded();
  const a = await rectOf(from, false);
  const b = await rectOf(to, true);
  await page.mouse.move(a.x + 2, a.y + a.h / 2);
  await page.mouse.down();
  await page.mouse.move(b.x + b.w - 2, b.y + b.h / 2, { steps: 8 });
  await page.mouse.up();
};
const button = (label) => page.locator(`button:text-is("${label}")`);

await page.click('.mm-shell-tab:has-text("Media")');
await page.evaluate((p) => window.__TAURI__.core.invoke("media_import", { paths: [p] }), clip);
await page.waitForSelector(".mm-media-row");
await page.click('.mm-shell-tab:has-text("Story")');
await page.waitForSelector(".mm-story-word");
await shot("1-reading");

// 区間ごとの先頭の単語の番号と、語数。
const assetId = await page.locator(".mm-story select[aria-label=Clip]").inputValue();
const { words } = await invoke("story_transcript", { asset: assetId });
const starts = words.flatMap((w, i) => (i === 0 || words[i - 1].segment !== w.segment ? [i] : []));
const range = (fromSeg, toSeg) => [starts[fromSeg], (starts[toSeg + 1] ?? words.length) - 1];

const makeTopic = async (fromSeg, toSeg, title) => {
  const [a, b] = range(fromSeg, toSeg);
  await drag(a, b);
  await page.fill('input[aria-label="Topic title"]', title);
  await button("Make topic").click();
  await page.waitForSelector(`.mm-story-topic-title:text-is("${title}")`);
};
await makeTopic(0, 0, "あいさつ");
await makeTopic(1, 2, "素材の取り込み");
await makeTopic(3, 3, "脱線 (ラーメン)");
await makeTopic(4, 6, "話題を選んで並べる");
await makeTopic(7, 8, "再生と書き出し");
await shot("2-topics");

// 言い淀みの単語を消し、読みを直す。
const wordOf = (text) => words.findIndex((w) => w.text === text);
const drop = wordOf("昼") >= 0 ? wordOf("昼") : starts[3] + 2;
await drag(drop, drop);
await page.waitForSelector('input[aria-label="Fixed text"]');
await shot("3-selected-word");
await button("Delete").click();
await page.waitForSelector('.mm-story-word[data-deleted="true"]');
const fixAt = starts[7] + 1;
await drag(fixAt, fixAt);
await page.fill('input[aria-label="Fixed text"]', "言いよどみ");
await page.locator('button:text-is("Fix")').click();
await page.waitForSelector('.mm-story-word[data-fixed="true"]');

// 評価とタグ。脱線は Skip、残りは Use。
const heads = page.locator(".mm-story-topic-head");
await heads.nth(2).locator('button:text-is("Skip")').click();
for (const i of [0, 1, 3, 4]) await heads.nth(i).locator('button:text-is("Use")').click();
await heads.nth(0).locator('input[aria-label="Add tag"]').fill("intro");
await heads.nth(0).locator('input[aria-label="Add tag"]').press("Enter");
await page.waitForSelector('.mm-story-tag:text-is("intro")');

// 全文検索。
await page.fill('input[aria-label="Search transcript"]', "文字起こし");
await page.press('input[aria-label="Search transcript"]', "Enter");
await page.waitForSelector(".mm-story-hit");
await shot("4-search");
await page.locator(".mm-story-hit").first().click();
await wait(300);
await page.fill('input[aria-label="Search transcript"]', "");

// Skip 以外の話題を、話した順とは違う並べ方で sequence に並べる。
const picks = page.locator('input[aria-label="Pick topic"]');
for (const i of [4, 0, 1, 3]) await picks.nth(i).check();
await shot("5-arrange");
await button("Arrange").click();
await page.waitForFunction(() => !document.querySelector(".mm-story-tray"));
await wait(500);
await shot("6-arranged");

// 並べたところを再生する。
const info = async () => invoke("project_info");
await invoke("transport", { action: "seek", value: 0 });
await invoke("transport", { action: "play", value: null });
await wait(2500);
await shot("7-playing");
const played = (await info()).layers;
console.log("sequence:", JSON.stringify((await invoke("story_overview")).sequences), "layers:", JSON.stringify(played));
await invoke("transport", { action: "pause", value: null });
await browser.close();
