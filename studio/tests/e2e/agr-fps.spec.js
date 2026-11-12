// AGR FPS is AGR's own rate and required in AGR mode: an empty box (saved as
// 0) blocks Start, and never falls back to Capture FPS. Boots the real app shell with AGR mode and Capture FPS 300 and
// checks every place the app shows or sends the AGR rate.
import { test, expect } from '@playwright/test';

async function boot(page, settings) {
  await page.addInitScript((s) => {
    window.__mockInvokeHandlers = {
      get_settings: () => ({
        capture_mode: 'agr', capture_fps: 300, target_drives: ['E:/caps'],
        ...s,
      }),
      calculate_export_pool_space: () => 500e9,
      system_memory_bytes: () => 16 * 1024 ** 3,
    };
  }, settings);
  await page.goto('/index.html');
  await expect(page.locator('#config-capture-mode')).toHaveValue('agr');
}

/** The `agr_fps` a batch started now would send; the paths are filled in
 *  only because the payload refuses to build without them. */
const sentAgrFps = (page) => page.evaluate(async () => {
  document.querySelector('#hl-path-input').value = 'C:/Games/Half-Life/hl.exe';
  document.querySelector('#hlae-path-input').value = 'C:/HLAE/HLAE.exe';
  const m = await import('/src/capture_pane.js');
  return m.batchSettingsPayload({ targetDrives: ['E:/caps'] }).agr_fps;
});

test('a blank AGR FPS blocks Start with the reason, and the box only hints', async ({ page }) => {
  await boot(page, { agr_fps: 0 });
  const box = page.locator('#config-agr-fps');
  await expect(box).toHaveValue('');
  await expect(box).toHaveAttribute('placeholder', 'e.g. 30');
  await expect(page.locator('#capture-summary-strip')).toContainText('AGR FPS not set');
  // Nothing is recorded at an invented rate: the batch is sent 0 and refused.
  expect(await sentAgrFps(page)).toBe(0);
  const guard = await page.evaluate(() => import('/src/capture_pane.js').then((m) => m.refreshLaunchGuard({
    targetDrives: ['E:/caps'],
    currentScannedDemos: [{ streaks: [{ selected: true }] }],
  })));
  expect(guard.agrFpsMissing).toBe(true);
  expect(guard.blocked).toBe(true);
  await expect(page.locator('#start-capture-btn')).toBeDisabled();
  await expect(page.locator('#disk-space-warning-banner')).toContainText('AGR FPS is empty');
  await expect(box).toHaveAttribute('aria-invalid', 'true');
  await expect(page.locator('.field-error-line[data-for-field="config-agr-fps"]')).toContainText('Set AGR FPS');

  // Typing a rate lifts the block.
  await box.evaluate((el) => { el.value = '30'; el.dispatchEvent(new Event('input', { bubbles: true })); });
  const after = await page.evaluate(() => import('/src/capture_pane.js').then((m) => m.refreshLaunchGuard({
    targetDrives: ['E:/caps'],
    currentScannedDemos: [{ streaks: [{ selected: true }] }],
  })));
  expect(after.agrFpsMissing).toBe(false);
  await expect(box).not.toHaveAttribute('aria-invalid', 'true');
});

test('other capture modes ignore a blank AGR FPS', async ({ page }) => {
  await boot(page, { agr_fps: 0 });
  await page.locator('#config-capture-mode').evaluate((el) => { el.value = 'frame_sequence'; el.dispatchEvent(new Event('change', { bubbles: true })); });
  const guard = await page.evaluate(() => import('/src/capture_pane.js').then((m) => m.refreshLaunchGuard({
    targetDrives: ['E:/caps'],
    currentScannedDemos: [{ streaks: [{ selected: true }] }],
  })));
  expect(guard.agrFpsMissing).toBe(false);
  await expect(page.locator('#disk-space-warning-banner')).not.toContainText('AGR FPS');
});

test('a set AGR FPS is shown and sent as it is', async ({ page }) => {
  await boot(page, { agr_fps: 120 });
  await expect(page.locator('#config-agr-fps')).toHaveValue('120');
  await expect(page.locator('#capture-summary-strip')).toContainText('AGR for Blender @ 120 fps');
  const sent = await sentAgrFps(page);
  expect(sent).toBe(120);
});
