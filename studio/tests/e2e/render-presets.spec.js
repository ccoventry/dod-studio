// render_presets_ui.js — named render setups (#108).
import { test, expect } from '@playwright/test';

async function loadHarness(page) {
  await page.goto('/tests/e2e/render-presets.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

test('saving names the current settings, and the select follows the fields', async ({ page }) => {
  await loadHarness(page);
  const select = page.locator('#render-preset-select');
  await expect(select).toHaveValue('');

  await page.locator('#render-codec-select').selectOption('h264');
  await page.locator('#render-fps-input').fill('240');
  await page.locator('#render-preset-name').fill('Discord');
  await page.locator('#render-preset-save').click();

  await expect(select).toHaveValue('Discord');
  expect(await page.evaluate(() => window.__getPresets())).toEqual([
    { name: 'Discord', codec: 'h264', custom_codec_args: '', fps: 240, max_concurrent: 2 },
  ]);
  expect(await page.evaluate(() => window.__saves)).toBe(1);

  // Editing a field away from the preset shows "—"; back again shows it.
  await page.locator('#render-fps-input').fill('300');
  await expect(select).toHaveValue('');
  await page.locator('#render-fps-input').fill('240');
  await expect(select).toHaveValue('Discord');
});

test('picking a preset applies all four settings', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => window.__setPresets([
    { name: 'Archive', codec: 'prores', fps: 300, max_concurrent: 1 },
    { name: 'Custom mpeg4', codec: 'custom', custom_codec_args: '-c:v mpeg4', fps: 120, max_concurrent: 4 },
  ]));
  await page.locator('#render-preset-select').selectOption('Custom mpeg4');
  await expect(page.locator('#render-codec-select')).toHaveValue('custom');
  await expect(page.locator('#render-custom-codec-input')).toHaveValue('-c:v mpeg4');
  await expect(page.locator('#render-fps-input')).toHaveValue('120');
  await expect(page.locator('#render-max-concurrent-input')).toHaveValue('4');
});

test('delete removes the picked preset and leaves the settings', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => window.__setPresets([{ name: 'Archive', codec: 'prores', fps: 300, max_concurrent: 2 }]));
  await expect(page.locator('#render-preset-select')).toHaveValue('Archive');
  await page.locator('#render-preset-delete').click();
  expect(await page.evaluate(() => window.__getPresets())).toEqual([]);
  await expect(page.locator('#render-preset-select')).toHaveValue('');
  await expect(page.locator('#render-codec-select')).toHaveValue('prores');
});
