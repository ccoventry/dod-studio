// crash_map_warnings.js — the ask before a batch on a map a session crashed
// on (#207).
import { test, expect } from '@playwright/test';

const HARRINGTON = (demo) => ({
  demo_name: demo, map: 'dod_harrington', count: 2, last_unix_secs: 1790000000, build: 'pre-Anniversary',
  cause: "The engine's movement trace ran on the previous map's collision data (#384).",
});

async function loadHarness(page, warnings) {
  await page.addInitScript((w) => {
    window.__mockInvokeHandlers = { crash_map_warnings: () => w };
  }, warnings);
  await page.goto('/tests/e2e/crash-maps.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

test('demos on a crash-prone map are grouped by map, and Cancel stops the batch', async ({ page }) => {
  await loadHarness(page, [HARRINGTON('a.dem'), HARRINGTON('b.dem'), HARRINGTON('c.dem')]);
  page.evaluate(() => window.__confirm(['C:/d/a.dem', 'C:/d/b.dem', 'C:/d/c.dem']));
  await expect(page.locator('#themed-confirm-title')).toHaveText('3 demos are on a map the game crashed on');
  const details = page.locator('#themed-confirm-details');
  await expect(details).toContainText('dod_harrington: a.dem, b.dem and 1 more');
  await expect(details).toContainText('#384');
  await expect(details).toContainText('Seen 2 times');
  await expect(details).toContainText('pre-Anniversary build');
  await expect(page.locator('#themed-confirm-ok-btn')).toHaveText('Start anyway');
  await page.click('#themed-confirm-cancel-btn');
  await expect(page.locator('#result')).toHaveText('false');
});

test('no remembered crash asks nothing', async ({ page }) => {
  await loadHarness(page, []);
  await page.evaluate(() => window.__confirm(['C:/d/a.dem']));
  await expect(page.locator('#result')).toHaveText('true');
  await expect(page.locator('#themed-confirm-modal')).toBeHidden();
});

test('a failing check never blocks the batch', async ({ page }) => {
  await page.addInitScript(() => {
    window.__mockInvokeHandlers = { crash_map_warnings: () => Promise.reject('boom') };
  });
  await page.goto('/tests/e2e/crash-maps.html');
  await page.waitForFunction(() => window.__harnessReady === true);
  await page.evaluate(() => window.__confirm(['C:/d/a.dem']));
  await expect(page.locator('#result')).toHaveText('true');
});
