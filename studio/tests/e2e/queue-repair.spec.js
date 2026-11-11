// queue_repair.js — a Master Queue demo damaged partway (#15) gets a
// "Damaged · Repair" button; clicking it repairs the demo without selecting
// the row, with a progress bar in its place until it ends.
import { test, expect } from '@playwright/test';

const row = (page, name) => page.locator('#master-demo-table-body tr', { hasText: name });

test.beforeEach(async ({ page }) => {
  await page.goto('/tests/e2e/queue-repair.html');
  await page.waitForFunction(() => window.__harnessReady === true);
});

test('only a damaged demo gets the button, which says where it breaks on hover', async ({ page }) => {
  const btn = row(page, 'broken.dem').locator('.damaged-repair-btn');
  await expect(btn).toHaveText('Damaged · Repair');
  await expect(btn).toHaveAttribute('title', /damaged at 8:27 on the playback clock/);
  await expect(row(page, 'fine.dem').locator('.damaged-repair-btn')).toHaveCount(0);
});

test('Repair runs without selecting the row, and shows its bar until it ends', async ({ page }) => {
  const r = row(page, 'broken.dem');
  await r.locator('.damaged-repair-btn').click();
  await expect(page.locator('#result')).toHaveText('repair broken.dem');
  await expect(r.locator('.repair-progress .split-progress-text')).toHaveText('Reading the recording… 40%');
  await expect(r.locator('.damaged-repair-btn')).toBeHidden();
  await page.evaluate(() => window.__finishRowRepair());
  await expect(r.locator('.repair-progress')).toHaveCount(0);
  await expect(r.locator('.damaged-repair-btn')).toBeVisible();
});
