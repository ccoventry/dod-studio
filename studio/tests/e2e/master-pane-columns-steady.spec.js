// The queue's columns stay put when a row turns missing or comes back, at the
// 1280 and 1920 windows (#1513). Driven against tests/e2e/master-pane-narrow.html.
import { test, expect } from '@playwright/test';

const HEADERS = ['Highlights', 'Selected', 'Status', 'Review', 'Actions'];

async function headerXs(page) {
  return page.evaluate((names) => {
    const ths = [...document.querySelectorAll('#master-demo-table th')];
    return names.map((n) => ths.find((t) => t.textContent.trim().startsWith(n)).getBoundingClientRect().x);
  }, HEADERS);
}

for (const [width, height] of [[1280, 720], [1920, 1080]]) {
  test(`columns do not move when a row turns missing at ${width}`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await page.goto('/tests/e2e/master-pane-narrow.html');
    await page.waitForFunction(() => window.__harnessReady === true);
    // The pane is the harness's fixed 538 px; at 1920 give it the wider split.
    await page.evaluate((w) => { document.querySelector('#pane').style.width = `${w === 1920 ? 900 : 538}px`; }, width);

    await page.evaluate(() => window.__render([window.__demos[0]]));
    const normal = await headerXs(page);

    await page.evaluate(() => window.__render([window.__demos[0], window.__demos[2]]));
    await expect(page.locator('.use-found-copy-btn')).toBeVisible();
    const withMissing = await headerXs(page);

    await page.evaluate(() => window.__render([window.__demos[0]]));
    const back = await headerXs(page);

    normal.forEach((x, i) => {
      expect(Math.abs(withMissing[i] - x), `${HEADERS[i]} moved when a row turned missing`).toBeLessThanOrEqual(1);
      expect(Math.abs(back[i] - x), `${HEADERS[i]} moved when the row came back`).toBeLessThanOrEqual(1);
    });
  });
}
