// Start Capture Batch's checks before the batch (#953): Start greys out and
// the footer line names each check from the click, and a check that stops
// the batch brings Start back with the reason on the line.
import { test, expect } from '@playwright/test';

async function loadHarness(page) {
  await page.addInitScript(() => {
    window.__mockInvokeHandlers = {
      // window.__holdDisk holds the disk check after the first pass.
      calculate_export_pool_space: () => (window.__holdDisk ? new Promise(() => {}) : 500e9),
      diagnose_capture_output_paths: () => [],
    };
  });
  await page.goto('/tests/e2e/capture-precheck.html');
  await page.waitForFunction(() => window.__harnessReady === true);
  // The disk-space guard's first pass lets Start go.
  await expect(page.locator('#start-capture-btn')).toBeEnabled();
}

test('the first line shows at once, before any check has answered', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => { window.__holdDisk = true; });
  await page.click('#start-capture-btn');
  await expect(page.locator('#start-capture-btn')).toBeDisabled();
  await expect(page.locator('#batch-status')).toHaveText('Checking the disk space…');
  await expect(page.locator('#capture-progress-container')).toHaveClass(/indeterminate/);
});

test('Start greys out and the line names each check while they run', async ({ page }) => {
  await loadHarness(page);
  await page.click('#start-capture-btn');
  await expect(page.locator('#start-capture-btn')).toBeDisabled();
  await expect(page.locator('#capture-progress-container')).toBeVisible();
  await expect(page.locator('#capture-progress-container')).toHaveClass(/indeterminate/);
  // Held at the demo check until the test settles it.
  await page.waitForFunction(() => window.__demosCalls === 1);
  await expect(page.locator('#batch-status')).toHaveText('Checking the picked demos are where they were…');
  await page.evaluate(() => window.__demosProgress({ done: 3, total: 12 }));
  await expect(page.locator('#batch-status')).toHaveText('Checking the picked demos are where they were (3 of 12)…');
  // A second click while it checks does nothing.
  await page.locator('#start-capture-btn').click({ force: true });
  expect(await page.evaluate(() => window.__demosCalls)).toBe(1);
  // Typing in a field that refreshes the guard leaves Start greyed out.
  await page.evaluate(() => import('/src/capture_pane.js').then((m) => m.refreshLaunchGuard()));
  await expect(page.locator('#start-capture-btn')).toBeDisabled();
});

test('a demo that is missing stops it: Start comes back and the line says why', async ({ page }) => {
  await loadHarness(page);
  await page.click('#start-capture-btn');
  await page.waitForFunction(() => window.__demosCalls === 1);
  await page.evaluate(() => window.__demosDone(false));
  await expect(page.locator('#batch-status')).toContainText('Capture not started: a demo with picked highlights is missing.');
  await expect(page.locator('#start-capture-btn')).toBeEnabled();
  await expect(page.locator('#capture-progress-container')).not.toHaveClass(/indeterminate/);
  await expect(page.locator('#capture-progress-container')).toBeHidden();
});

test('a question answered no says it was not started', async ({ page }) => {
  await loadHarness(page);
  await page.click('#start-capture-btn');
  await page.waitForFunction(() => window.__demosCalls === 1);
  // 'handled': the demo check asked something and the answer was no.
  await page.evaluate(() => window.__demosDone('handled'));
  await expect(page.locator('#batch-status')).toHaveText('Not started.');
  await expect(page.locator('#start-capture-btn')).toBeEnabled();
});

// #1097: a batch the backend refuses once it plans the queue shows one plain
// error toast, with no "queued" toast before it.
const emit = (page, payload) => page.evaluate((p) => window.__mockEmit('capture_status', p), payload);

test('a refused batch shows only its error, in plain words, once', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => { window.__mockInvokeHandlers.steam_state = () => 'ready'; });
  await page.click('#start-capture-btn');
  await page.waitForFunction(() => window.__demosCalls === 1);
  await page.evaluate(() => window.__demosDone(true));
  await page.waitForFunction(() => window.__mockInvocations.some((c) => c.cmd === 'start_capture_batch'));
  const refusal = {
    running: false,
    error: true,
    status: 'Not enough free space on any capture drive for this batch: a clip needs 31.0 GB free on one drive, and the most any one has is 22.0 GB.',
  };
  await emit(page, { running: true, stage: 'preparing', index: 0, total: 0, status: 'Making room…' });
  await emit(page, refusal);
  await expect(page.locator('.toast-error')).toHaveCount(1);
  await expect(page.locator('.toast-error')).toHaveText(`Capture error: ${refusal.status}`);
  // Every toast is logged as it shows (#873), so one that came and went counts too.
  const toasts = await page.evaluate(() => window.__mockInvocations
    .filter((c) => c.cmd === 'log_frontend_event')
    .map((c) => JSON.stringify(c.args))
    .filter((a) => a.includes('[toast ')));
  expect(toasts.filter((a) => a.includes('[toast success]'))).toEqual([]);
  expect(toasts.filter((a) => a.includes('[toast error]'))).toHaveLength(1);
});
