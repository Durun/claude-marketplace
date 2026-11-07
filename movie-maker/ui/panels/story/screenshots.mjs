import { mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const outDir = fileURLToPath(new URL("./screenshots/", import.meta.url));

const dragWords = async (page, from, to) => {
  const a = await page.locator(`[data-i="${from}"]`).boundingBox();
  const b = await page.locator(`[data-i="${to}"]`).boundingBox();
  await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
  await page.mouse.down();
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 6 });
  await page.mouse.up();
};

/** モックの API で状態を作り、左の列と同じ幅で撮る。 */
export default async (browser, { dist }) => {
  await mkdir(outDir, { recursive: true });
  for (const theme of ["light", "dark"]) {
    const open = async (state) => {
      const page = await browser.newPage({ viewport: { width: 282, height: 760 }, deviceScaleFactor: 2 });
      await page.goto(`${dist("story-demo.html")}?theme=${theme}&state=${state}`);
      await page.waitForSelector(".mm-story-word");
      return page;
    };
    const shot = async (page, name) => {
      await page.mouse.move(0, 0);
      await page.screenshot({ path: `${outDir}story-${theme}-${name}.png` });
    };
    const plain = await open("plain");
    await shot(plain, "reading");
    await dragWords(plain, 11, 17);
    await shot(plain, "selection");
    await plain.close();
    const topics = await open("topics");
    await shot(topics, "topics");
    const picks = topics.locator('input[aria-label="Pick topic"]');
    for (const i of [3, 0, 1]) await picks.nth(i).check();
    await shot(topics, "arrange");
    await topics.fill('input[aria-label="Search transcript"]', "ます");
    await topics.press('input[aria-label="Search transcript"]', "Enter");
    await topics.waitForSelector(".mm-story-hit");
    await shot(topics, "search");
    await topics.close();
  }
};
