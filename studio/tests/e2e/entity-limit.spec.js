// packet_entity_limit.js — demos the game's engine can't play (#207).
//
// Pins the Master Queue mark (only over the engine's limit, only when the
// count is known), that an Anniversary engine's 1024 drops the mark, and the
// ask before a batch or a preview: listed demos, Cancel stops, and nothing
// over the limit asks nothing.
import { test, expect } from '@playwright/test';

async function loadHarness(page, engineLimit = null) {
  await page.addInitScript((limit) => {
    window.__mockInvokeHandlers = window.__mockInvokeHandlers || {};
    window.__mockInvokeHandlers.engine_packet_entity_limit = () => limit;
  }, engineLimit);
  await page.goto('/tests/e2e/entity-limit.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const rows = (page) => page.locator('#master-demo-table-body tr');

test('only a demo over 256 is marked, and its tooltip names both engines', async ({ page }) => {
  await loadHarness(page);
  await expect(rows(page)).toHaveCount(3);
  const badges = page.locator('.entity-limit-badge');
  await expect(badges).toHaveCount(1);
  await expect(rows(page).nth(0).locator('.entity-limit-badge')).toHaveText("won't play");
  await expect(badges).toHaveAttribute('title', /Up to 301 entities.*more than 256.*25th Anniversary engine allows 1024/);
});

test("the 25th Anniversary engine's 1024 drops the mark", async ({ page }) => {
  await loadHarness(page, 1024);
  await page.evaluate(() => window.__rerender('C:/games/POST/hl.exe'));
  await expect(page.locator('.entity-limit-badge')).toHaveCount(0);
});

test('a batch over the limit asks first, lists the demo, and Cancel stops it', async ({ page }) => {
  await loadHarness(page);
  page.evaluate(() => window.__confirm(['lennon2_hltv.dem', 'anzio.dem'], 'C:/games/PRE/hl.exe', false));
  await expect(page.locator('#themed-confirm-modal')).toBeVisible();
  await expect(page.locator('#themed-confirm-title')).toHaveText("1 demo won't play in this game");
  await expect(page.locator('#themed-confirm-details')).toContainText('lennon2_hltv.dem');
  await expect(page.locator('#themed-confirm-details')).not.toContainText('anzio.dem');
  await expect(page.locator('#themed-confirm-footer')).toContainText('25th Anniversary');
  await expect(page.locator('#themed-confirm-ok-btn')).toHaveText('Start anyway');
  await page.click('#themed-confirm-cancel-btn');
  await expect(page.locator('#result')).toHaveText('false');

  page.evaluate(() => window.__confirm(['lennon2_hltv.dem'], 'C:/games/PRE/hl.exe', true));
  await expect(page.locator('#themed-confirm-title')).toHaveText("This demo won't play in this game");
  await page.click('#themed-confirm-ok-btn');
  await expect(page.locator('#result')).toHaveText('true');
});

test('nothing over the limit asks nothing', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => window.__confirm(['anzio.dem', 'old.dem'], 'C:/games/PRE/hl.exe', false));
  await expect(page.locator('#result')).toHaveText('true');
  await expect(page.locator('#themed-confirm-modal')).toBeHidden();
});
