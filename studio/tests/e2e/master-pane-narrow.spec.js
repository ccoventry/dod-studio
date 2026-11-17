// The queue's Locate and Use found copy buttons stay inside the pane at a
// 1280-wide window, even with a very long demo name (#1376). Driven against
// tests/e2e/master-pane-narrow.html. See tests/e2e/README.md.
import { test, expect } from '@playwright/test';

test.use({ viewport: { width: 1280, height: 720 } });

const row = (page, text) => page.locator('#master-demo-table-body tr', { hasText: text });

test.describe('missing demo row in a narrow queue pane', () => {
  test('Locate, Use found copy and Remove end inside the pane and the viewport, without scrolling', async ({ page }) => {
    await page.goto('/tests/e2e/master-pane-narrow.html');
    await page.waitForFunction(() => window.__harnessReady === true);
    const wrapper = await page.locator('.table-wrapper').boundingBox();
    const scroll = await page.locator('.table-wrapper').evaluate((e) => [e.scrollWidth, e.clientWidth]);
    expect(scroll[0]).toBe(scroll[1]);
    for (const text of ['gone.dem', 'harrington_vs']) {
      for (const sel of ['.locate-demo-btn', '.list-editor-remove-btn']) {
        const box = await row(page, text).locator(sel).boundingBox();
        expect(box.x + box.width).toBeLessThanOrEqual(wrapper.x + wrapper.width);
        expect(box.x + box.width).toBeLessThanOrEqual(1280);
      }
    }
    const found = await row(page, 'harrington_vs').locator('.use-found-copy-btn').boundingBox();
    expect(found.x + found.width).toBeLessThanOrEqual(wrapper.x + wrapper.width);
    await expect(row(page, 'harrington_vs').locator('.locate-demo-btn')).toBeVisible();
  });

  test('a long name is cut with an ellipsis and its tooltip gives the full path', async ({ page }) => {
    await page.goto('/tests/e2e/master-pane-narrow.html');
    await page.waitForFunction(() => window.__harnessReady === true);
    const name = row(page, 'harrington_vs').locator('.queue-name-cell span').first();
    const cut = await name.evaluate((e) => e.scrollWidth > e.clientWidth);
    expect(cut).toBe(true);
    await expect(name).toHaveAttribute('title', /very\/deep\/folder.*2026-11-12\.dem$/);
    expect((await name.boundingBox()).width).toBeGreaterThan(40);
  });

  // The Capture split lets the queue pane go to 380 px. A button cut off by
  // the name cell's clip still sits inside the wrapper's box, so compare with
  // the cell, and with the page for a button the wrapper scrolls to (#1513).
  for (const paneWidth of [538, 462, 380]) {
    test(`Use found copy and Locate are whole inside the name cell at a ${paneWidth} px pane`, async ({ page }) => {
      await page.goto('/tests/e2e/master-pane-narrow.html');
      await page.waitForFunction(() => window.__harnessReady === true);
      await page.evaluate((w) => { document.querySelector('#pane').style.width = `${w}px`; }, paneWidth);
      for (const text of ['gone.dem', 'harrington_vs']) {
        const cell = (await row(page, text).locator('.queue-name-cell').boundingBox());
        for (const sel of ['.locate-demo-btn', '.use-found-copy-btn']) {
          const btn = row(page, text).locator(sel);
          if ((await btn.count()) === 0) continue;
          const box = await btn.boundingBox();
          expect(box.x + box.width, `${text} ${sel} right edge`).toBeLessThanOrEqual(cell.x + cell.width + 0.5);
          expect(box.x, `${text} ${sel} left edge`).toBeGreaterThanOrEqual(cell.x - 0.5);
        }
      }
    });
  }
});
