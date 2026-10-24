// The update and map downloads' step lines and bars (#957), driven against
// tests/e2e/downloads.html with the backend's events mocked.
import { test, expect } from '@playwright/test';

const MB = 1024 * 1024;

async function gotoHarness(page) {
  await page.goto('/tests/e2e/downloads.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

test('the update shows its MB, then a sliding bar while it installs, then Ready', async ({ page }) => {
  await gotoHarness(page);
  const status = page.locator('#update-status-text');
  const bar = page.locator('#update-progress-bar');

  await page.evaluate((mb) => window.__mockEmit('update_download_progress', { downloaded: 12 * mb, total: 45 * mb }), MB);
  await expect(status).toHaveText('Downloading the update: 12 of 45 MB');
  await expect(page.locator('#update-progress-container')).toBeVisible();
  expect(await bar.evaluate((b) => [b.value, b.max])).toEqual([12 * MB, 45 * MB]);

  await page.evaluate(() => window.__mockEmit('update_installing', null));
  await expect(status).toHaveText('Installing the update… DoD Studio closes while it installs.');
  // No value: the browser's own sliding block.
  expect(await bar.evaluate((b) => b.hasAttribute('value'))).toBe(false);

  await page.evaluate(() => window.__mockEmit('update_ready', null));
  await expect(status).toHaveText('Ready: restart to finish.');
  await expect(page.locator('#restart-to-apply-btn')).toBeVisible();
  await expect(page.locator('#update-progress-container')).toBeHidden();
});

test('Download all names each map, counts them, shows its MB and the check', async ({ page }) => {
  await gotoHarness(page);
  await page.locator('#map-download-all-btn').click();
  const line = page.locator('#map-download-line');
  const bar = page.locator('#map-download-bar');

  await expect(line).toHaveText('Downloading dod_x (1 of 2)…');
  expect(await bar.evaluate((b) => b.hasAttribute('value'))).toBe(false);

  await page.evaluate((mb) => window.__mockEmit('map_download_progress', { map: 'dod_x', step: 'downloading', downloaded: 2.1 * mb, total: 6 * mb }), MB);
  await expect(line).toHaveText('Downloading dod_x (1 of 2): 2.1 of 6 MB');
  expect(await bar.evaluate((b) => [b.value, b.max])).toEqual([2.1 * MB, 6 * MB]);

  // Another map's event is not this one's.
  await page.evaluate(() => window.__mockEmit('map_download_progress', { map: 'dod_y', step: 'checking', downloaded: 1, total: 1 }));
  await expect(line).toHaveText('Downloading dod_x (1 of 2): 2.1 of 6 MB');

  await page.evaluate((mb) => window.__mockEmit('map_download_progress', { map: 'dod_x', step: 'checking', downloaded: 6 * mb, total: 6 * mb }), MB);
  await expect(line).toHaveText('Checking dod_x…');
  expect(await bar.evaluate((b) => b.hasAttribute('value'))).toBe(false);
  await expect(page.locator('#map-download-all-btn')).toBeDisabled();

  await page.evaluate(() => window.__finishMap());
  await expect(line).toHaveText('Downloading dod_y (2 of 2)…');

  await page.evaluate(() => window.__finishMap());
  // Both installed: the banner has nothing left to show.
  await expect(page.locator('#map-warning-banner')).toBeHidden();
});
