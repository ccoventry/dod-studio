// pinned_review.js (#533), driven against tests/e2e/pinned-review.html.
import { test, expect } from '@playwright/test';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/pinned-review.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

test('lists every pinned folder ticked, and unpins only the unticked ones', async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate(() => window.__start(['D:/demos', 'D:/test stuff', 'E:/matches']));
  const modal = page.locator('#pinned-review-modal');
  await expect(modal).toBeVisible();
  await expect(modal).toContainText('These may have been pinned automatically by + Add Folder');
  const boxes = modal.locator('input[type="checkbox"]');
  await expect(boxes).toHaveCount(3);
  for (let i = 0; i < 3; i += 1) await expect(boxes.nth(i)).toBeChecked();

  await modal.locator('label', { hasText: 'D:/test stuff' }).locator('input').uncheck();
  await page.click('#pinned-review-save-btn');
  await page.evaluate(() => window.__done);
  await expect(modal).toHaveCount(0);
  expect(await page.evaluate(() => window.__unpinned)).toEqual(['D:/test stuff']);
  expect(await page.evaluate(() => window.__reviewed())).toBe(true);
  expect(await page.evaluate(() => window.__saves)).toBe(1);
});

test('Keep all keeps every pin and still saves that the look ran', async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate(() => window.__start(['D:/demos', 'E:/matches']));
  await page.locator('#pinned-review-modal input').first().uncheck();
  await page.click('#pinned-review-keep-all-btn');
  await page.evaluate(() => window.__done);
  expect(await page.evaluate(() => window.__unpinned)).toBe(null);
  expect(await page.evaluate(() => window.__reviewed())).toBe(true);
  expect(await page.evaluate(() => window.__saves)).toBe(1);
});
