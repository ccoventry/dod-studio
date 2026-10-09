// demo_cache.js (#569), driven against tests/e2e/demo-cache.html.
import { test, expect } from '@playwright/test';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/demo-cache.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

test('Cache all sends the folder\'s demos, follows progress, and says how it went', async ({ page }) => {
  await gotoHarness(page);
  const button = page.locator('#analyzer-cache-all-btn');
  const status = page.locator('#analyzer-cache-status');

  await button.click();
  const calls = await page.evaluate(() => window.__mockInvocations.filter((c) => c.cmd === 'cache_demos'));
  expect(calls).toHaveLength(1);
  expect(calls[0].args.paths).toEqual(['C:\demos\a.dem', 'C:\demos\b.dem', 'C:\demos\c.dem']);
  await expect(button).toHaveText('Stop');
  await expect(status).toHaveText('Caching 0 / 3');

  await page.evaluate(() => window.__mockEmit('demo_cache_progress', { done: 2, total: 3, already: 1, failed: 0, finished: false, cancelled: false }));
  await expect(status).toHaveText('Caching 2 / 3 (1 already cached)');

  await page.evaluate(() => window.__mockEmit('demo_cache_progress', { done: 3, total: 3, already: 1, failed: 1, finished: true, cancelled: false }));
  await expect(status).toHaveText('Cached 3 demos (1 already cached, 1 failed)');
  await expect(button).toHaveText('Cache all');
});

test('Stop asks the backend to stop, and a stopped run says where it stopped', async ({ page }) => {
  await gotoHarness(page);
  const button = page.locator('#analyzer-cache-all-btn');
  await button.click();
  await button.click();
  const stops = await page.evaluate(() => window.__mockInvocations.filter((c) => c.cmd === 'cancel_demo_cache'));
  expect(stops).toHaveLength(1);

  await page.evaluate(() => window.__mockEmit('demo_cache_progress', { done: 1, total: 3, already: 0, failed: 0, finished: true, cancelled: true }));
  await expect(page.locator('#analyzer-cache-status')).toHaveText('Stopped at 1 / 3');
  await expect(button).toHaveText('Cache all');
});

test('an empty folder says so and starts nothing', async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate(() => { window.__demoPaths = []; });
  await page.locator('#analyzer-cache-all-btn').click();
  await expect(page.locator('#analyzer-cache-status')).toHaveText('No demos in this folder to cache.');
  const calls = await page.evaluate(() => window.__mockInvocations.filter((c) => c.cmd === 'cache_demos'));
  expect(calls).toHaveLength(0);
});
