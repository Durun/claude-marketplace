import { mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const outDir = fileURLToPath(new URL("./screenshots/", import.meta.url));
const SCENES = ["idle", "importing", "failed", "empty", "cache"];

/** モックの API で状態を作り、左の列と同じ幅で撮る。 */
export default async (browser, { dist }) => {
  await mkdir(outDir, { recursive: true });
  for (const theme of ["light", "dark"]) {
    for (const scene of SCENES) {
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
