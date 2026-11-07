import { mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const outDir = fileURLToPath(new URL("./screenshots/", import.meta.url));

/** Inspector のキーフレームナビゲーターと Graph。ドラッグの途中（離す前）も撮る。 */
export default async (browser, { dist }) => {
  await mkdir(outDir, { recursive: true });
  for (const theme of ["light", "dark"]) {
    const page = await browser.newPage({ viewport: { width: 1000, height: 640 }, deviceScaleFactor: 2 });
    await page.goto(`${dist("keyframes-demo.html")}?theme=${theme}&t=2`);
    await page.waitForSelector(".mm-graph-canvas");
    await page.screenshot({ path: `${outDir}keyframes-${theme}-position.png` });
    await page.locator(".mm-insp-pick", { hasText: "opacity" }).click();
    await page.screenshot({ path: `${outDir}keyframes-${theme}-opacity.png` });
    const box = await page.locator('[aria-label="key 1.000s x"]').boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + 80, box.y + 120, { steps: 4 });
    await page.waitForTimeout(100);
    await page.screenshot({ path: `${outDir}keyframes-${theme}-dragging.png` });
    await page.mouse.up();
    await page.screenshot({ path: `${outDir}keyframes-${theme}-dropped.png` });
    await page.close();
  }
  // 本体の画面: 右の列の Inspector と、その下の Graph。
  const page = await browser.newPage({ viewport: { width: 900, height: 900 }, deviceScaleFactor: 2 });
  await page.goto(`${dist("index.html")}?theme=light`);
  await page.waitForSelector(".mm-insp");
  await page.waitForTimeout(200);
  await page.evaluate(() => {
    const t = (num) => ({ num, den: 1 });
    const n = (value) => ({ type: "number", value });
    const keys = [
      { time: t(0), value: n(0), interpolation: "bezier" },
      { time: t(1), value: n(1), interpolation: "linear", ease: "in_out" },
      { time: t(3), value: n(0.3), interpolation: "linear" },
    ];
    const layer = {
      id: "title", start: t(0), in: t(0), out: t(4),
      opacity: { value: n(1), keyframes: keys },
      transform: { position: { value: { type: "vec2", value: [0, 0] } }, scale: { value: n(1) }, rotation: { value: n(0) } },
      source: { kind: "text", version: 1, params: {} },
    };
    window.dispatchEvent(new CustomEvent("mm:layer-selected", { detail: { layer, assets: [] } }));
    window.dispatchEvent(new CustomEvent("mm:playhead", { detail: { time: 1, fps: 30 } }));
  });
  await page.locator(".mm-insp-pick", { hasText: "opacity" }).click();
  await page.screenshot({ path: `${outDir}shell-light.png` });
  await page.close();
};
