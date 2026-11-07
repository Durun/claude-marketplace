import { mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const outDir = fileURLToPath(new URL("./screenshots/", import.meta.url));

/** モックの API で、保留 2 件（1 件は一部だけチェック、1 件は conflict あり）と決定済み 2 件を撮る。右の列と同じ幅。 */
export default async (browser, { dist }) => {
  await mkdir(outDir, { recursive: true });
  for (const theme of ["light", "dark"]) {
    const page = await browser.newPage({ viewport: { width: 322, height: 800 }, deviceScaleFactor: 2 });
    await page.goto(`${dist("review-demo.html")}?theme=${theme}`);
    await page.waitForSelector(".mm-review-card");
    await page.locator('input[type="checkbox"]').nth(2).uncheck();
    await page.getByRole("button", { name: "Proposed" }).first().click();
    await page.mouse.move(0, 0);
    await page.screenshot({ path: `${outDir}review-${theme}.png` });
    await page.close();
  }
};
