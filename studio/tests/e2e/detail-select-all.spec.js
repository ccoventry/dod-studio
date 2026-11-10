// Highlight Details' header checkbox: ticking or unticking every row marks
// the project unsaved, as a row's own checkbox does. Driven against
// tests/e2e/detail-notes.html (one demo, two highlights). See tests/e2e/README.md.
import { test, expect } from '@playwright/test';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/detail-notes.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const headerCb = (page) => page.locator('#detail-select-all-cb');
const dirtyCount = (page) => page.evaluate(() => window.__dirty);
const selected = (page) => page.evaluate(() => window.__demo.streaks.map((s) => s.selected));

test.describe('Highlight Details header checkbox', () => {
  test('ticking all rows marks the project unsaved once', async ({ page }) => {
    await gotoHarness(page);
    await headerCb(page).check();
    expect(await selected(page)).toEqual([true, true]);
    expect(await dirtyCount(page)).toBe(1);
  });

  test('unticking all rows marks the project unsaved', async ({ page }) => {
    await gotoHarness(page);
    await headerCb(page).check();
    await headerCb(page).uncheck();
    expect(await selected(page)).toEqual([false, false]);
    expect(await dirtyCount(page)).toBe(2);
  });

  test('with one row already ticked, ticking all still marks it unsaved', async ({ page }) => {
    await gotoHarness(page);
    await page.locator('#detail-streaks-table .streak-select-cb').first().check();
    expect(await dirtyCount(page)).toBe(1);
    await headerCb(page).check();
    expect(await selected(page)).toEqual([true, true]);
    expect(await dirtyCount(page)).toBe(2);
  });
});
