import { mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const outDir = fileURLToPath(new URL("./screenshots/", import.meta.url));

/** モックの API で、一覧、編集、書き出しの途中、終わったあとを撮る。書き出しは `__deliverStep` で進める。 */
export default async (browser, { dist }) => {
  await mkdir(outDir, { recursive: true });
  for (const theme of ["light", "dark"]) {
    const page = await browser.newPage({ viewport: { width: 282, height: 900 }, deviceScaleFactor: 2 });
    const shot = async (name) => {
      await page.mouse.move(0, 0);
      await page.screenshot({ path: `${outDir}deliver-${theme}-${name}.png` });
    };
    await page.goto(`${dist("deliver-demo.html")}?theme=${theme}`);
    await page.waitForSelector(".mm-deliver-row");
    await shot("targets");
    await page.getByRole("button", { name: "Edit" }).nth(1).click();
    await shot("edit");
    await page.getByRole("button", { name: "Cancel" }).click();
    await page.getByLabel("Queue youtube-1080p").check();
    await page.getByLabel("Queue youtube-shorts").check();
    await shot("queue");
    await page.getByRole("button", { name: "Export 2 targets" }).click();
    await page.waitForSelector(".mm-deliver-item");
    await page.evaluate(() => window.__deliverStep(1260));
    await page.evaluate(() => window.__deliverStep(420));
    await page.waitForSelector("[role=progressbar]");
    await shot("running");
    await page.evaluate(() => window.__deliverStep(1260));
    await page.waitForSelector(".mm-deliver-queue h3:has-text('Export finished')");
    await shot("done");
    await page.close();
  }
};
