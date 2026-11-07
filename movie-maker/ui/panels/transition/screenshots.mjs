import { mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const outDir = fileURLToPath(new URL("./screenshots/", import.meta.url));

/** モックの API で、トランジションを置いたカットを選んだ状態と、何も置いていない状態を左の列の幅で撮る。 */
export default async (browser, { dist }) => {
  await mkdir(outDir, { recursive: true });
  for (const theme of ["light", "dark"]) {
    const open = async (state) => {
      const page = await browser.newPage({ viewport: { width: 320, height: 300 }, deviceScaleFactor: 2 });
      await page.goto(`${dist("transition-demo.html")}?theme=${theme}&state=${state}`);
      await page.waitForSelector(".mm-tr-cut");
      return page;
    };
    const placed = await open("placed");
    await placed.locator(".mm-tr-cut", { hasText: "k4" }).click();
    await placed.mouse.move(0, 0);
    await placed.screenshot({ path: `${outDir}transition-${theme}-placed.png` });
    await placed.close();
    const plain = await open("plain");
    await plain.locator(".mm-tr-cut", { hasText: "k3" }).click();
    await plain.selectOption('[aria-label="Transition kind"]', "dissolve");
    await plain.waitForSelector(".mm-tr-mark");
    await plain.mouse.move(0, 0);
    await plain.screenshot({ path: `${outDir}transition-${theme}-plain.png` });
    await plain.close();
  }
};
