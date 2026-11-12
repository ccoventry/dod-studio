// Start Capture's blockers mark the Configuration field that causes them (#25):
// a blank hl.exe or hlae.exe path, no Capture Output folder. Harness: the
// capture precheck page, which carries those three fields.
import { test, expect } from './fixtures.js';

async function loadHarness(page) {
  await page.addInitScript(() => {
    window.__mockInvokeHandlers = {
      calculate_export_pool_space: () => 500e9,
      diagnose_capture_output_paths: () => [],
    };
  });
  await page.goto('/tests/e2e/capture-precheck.html');
  await page.waitForFunction(() => window.__harnessReady === true);
  await expect(page.locator('#start-capture-btn')).toBeEnabled();
}

const refresh = (page) => page.evaluate(() => import('/src/capture_pane.js').then((m) => m.refreshLaunchGuard()));
const lineUnder = (page, sel) => page.locator(sel).evaluate((el) => {
  const line = (el.closest('.input-row') || el).nextElementSibling;
  return line?.classList.contains('field-error-line') && !line.hidden ? line.textContent : '';
});

test('a blank hlae.exe path marks its field, and setting it clears the mark', async ({ page }) => {
  await loadHarness(page);
  await page.fill('#hlae-path-input', '');
  await refresh(page);
  await expect(page.locator('#hlae-path-input')).toHaveClass(/field-invalid/);
  expect(await lineUnder(page, '#hlae-path-input')).toContain('hlae.exe');
  await page.fill('#hlae-path-input', 'C:/HLAE/hlae.exe');
  await refresh(page);
  await expect(page.locator('#hlae-path-input')).not.toHaveClass(/field-invalid/);
  expect(await lineUnder(page, '#hlae-path-input')).toBe('');
});

test('a blank hl.exe path marks the install list', async ({ page }) => {
  await loadHarness(page);
  await page.fill('#hl-path-input', '');
  await refresh(page);
  await expect(page.locator('#hl-install-choice')).toHaveClass(/field-invalid/);
  expect(await lineUnder(page, '#hl-install-choice')).toContain('hl.exe');
});

test('no Capture Output folder marks the folder field', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => { window.__state.targetDrives = []; });
  await refresh(page);
  await expect(page.locator('#drive-path-input')).toHaveClass(/field-invalid/);
  expect(await lineUnder(page, '#drive-path-input')).toContain('Capture Output folder');
  await page.evaluate(() => { window.__state.targetDrives = ['D:/captures']; });
  await refresh(page);
  await expect(page.locator('#drive-path-input')).not.toHaveClass(/field-invalid/);
});

test('nothing is marked while no highlight is picked', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => { window.__state.currentScannedDemos = []; window.__state.targetDrives = []; });
  await page.fill('#hlae-path-input', '');
  await refresh(page);
  await expect(page.locator('.field-invalid')).toHaveCount(0);
});
