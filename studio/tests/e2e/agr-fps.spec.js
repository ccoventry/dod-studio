// AGR FPS is AGR's own rate: an empty box (saved as 0) records at 30, never at
// Capture FPS. Boots the real app shell with AGR mode and Capture FPS 300 and
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

test('an unset AGR FPS shows 30 and leaves the box empty, whatever Capture FPS says', async ({ page }) => {
  await boot(page, { agr_fps: 0 });
  const box = page.locator('#config-agr-fps');
  await expect(box).toHaveValue('');
  await expect(box).toHaveAttribute('placeholder', '30');
  await expect(page.locator('#capture-summary-strip')).toContainText('AGR for Blender @ 30 fps');
  // The batch is sent 0, which the backend reads as 30 (effective_agr_fps).
  const sent = await sentAgrFps(page);
  expect(sent).toBe(0);
  const rate = await page.evaluate(() => import('/src/take_record.js')
    .then((m) => m.captureRateOf('agr', { capture_fps: 300, agr_fps: 0 })));
  expect(rate).toBe(30);
});

test('a set AGR FPS is shown and sent as it is', async ({ page }) => {
  await boot(page, { agr_fps: 120 });
  await expect(page.locator('#config-agr-fps')).toHaveValue('120');
  await expect(page.locator('#capture-summary-strip')).toContainText('AGR for Blender @ 120 fps');
  const sent = await sentAgrFps(page);
  expect(sent).toBe(120);
});
