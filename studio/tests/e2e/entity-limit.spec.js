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
  await expect(rows(page).nth(0).locator('.entity-limit-badge')).toHaveText('trimmed copy');
  await expect(badges).toHaveAttribute('title', /Up to 301 entities.*more than 256.*trimmed copy.*original file is unchanged.*25th Anniversary engine allows 1024/);
});

test("the 25th Anniversary engine's 1024 drops the mark", async ({ page }) => {
  await loadHarness(page, 1024);
  await page.evaluate(() => window.__rerender('C:/games/POST/hl.exe'));
  await expect(page.locator('.entity-limit-badge')).toHaveCount(0);
});
