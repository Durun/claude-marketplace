import { mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const outDir = fileURLToPath(new URL("./screenshots/", import.meta.url));

/** モックの API で、メーターが振れる状態と、ダッキング有効でミュートとソロのある状態を撮る。あわせて本体の左右のタブも撮る。 */
export default async (browser, { dist }) => {
  await mkdir(outDir, { recursive: true });
  for (const theme of ["light", "dark"]) {
    for (const scene of ["meters", "ducking"]) {
      const page = await browser.newPage({ viewport: { width: 320, height: 900 }, deviceScaleFactor: 2 });
      await page.goto(`${dist("mixer-demo.html")}?theme=${theme}&scene=${scene}`);
      await page.waitForSelector(".mm-mixer-track");
      await page.waitForTimeout(700);
      await page.mouse.move(0, 0);
      await page.screenshot({ path: `${outDir}mixer-${theme}-${scene}.png`, fullPage: true });
      await page.close();
    }
  }
  const page = await browser.newPage({ viewport: { width: 1100, height: 600 } });
  await page.goto(`${dist("index.html")}?theme=light`);
  await page.waitForSelector(".mm-shell-tab");
  await page.screenshot({ path: `${outDir}shell-light.png` });
  await page.close();
};
