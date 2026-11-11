// Start Capture Batch with a highlight picked in a demo that recorded more
// than one map (#217): the batch starts like any other, on the demo's first
// map, with no question and no split, and the activity log says so.
import { test, expect } from '@playwright/test';

test('a pick in a multi-map demo starts the batch without asking, and the log says the first map is used', async ({ page }) => {
  await page.addInitScript(() => {
    window.__mockInvokeHandlers = {
      calculate_export_pool_space: () => 500e9,
      diagnose_capture_output_paths: () => [],
      steam_state: () => 'ready',
    };
  });
  await page.goto('/tests/e2e/capture-multimap.html');
  await page.waitForFunction(() => window.__harnessReady === true);
  await expect(page.locator('#start-capture-btn')).toBeEnabled();

  await page.click('#start-capture-btn');
  await page.waitForFunction(() => window.__mockInvocations.some((c) => c.cmd === 'start_capture_batch'));

  await expect(page.locator('#themed-confirm-modal')).toHaveCount(0);
  const calls = await page.evaluate(() => window.__mockInvocations.map((c) => c.cmd));
  expect(calls).not.toContain('split_demo_auto');
  const log = await page.evaluate(() => window.__mockInvocations
    .filter((c) => c.cmd === 'log_frontend_event')
    .map((c) => c.args.message));
  expect(log).toContain('[capture] Using the first map of 1 demo that recorded more than one map');
});
