import { mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const outDir = fileURLToPath(new URL("./screenshots/", import.meta.url));

/** 本体の画面の左の列に載った Tasks を、開いた状態と畳んだ状態で撮る。 */
export default async (browser, { dist }) => {
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
