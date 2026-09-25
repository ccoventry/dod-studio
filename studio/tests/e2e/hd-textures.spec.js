// hd_pane.js — the HD Textures page (#372).
//
// Pins what the page shows for a status report (the per-type table, the style
// picker and the movie.cfg lines built from the backend's cvar names), that a
// missing game path reads as a message rather than a broken page, and that the
// download's progress and cancel land in the progress line.
import { test, expect } from '@playwright/test';

/** native::hd::HdStatus, as serde sends it (snake_case). */
const STATUS = {
  hd_root: 'C:/games/Half-Life/dod/dodstudio_hd',
  hd_root_exists: true,
  types: [
    {
      asset_type: 'world',
      folders: [
        { name: 'overrides', files: 0, bytes: 0 },
        { name: 'plain', files: 4708, bytes: 3 * 1024 ** 3 },
      ],
    },
    { asset_type: 'models', folders: [] },
    { asset_type: 'sprites', folders: [] },
    { asset_type: 'detail', folders: [{ name: 'plain', files: 493, bytes: 50 * 1024 ** 2 }] },
    { asset_type: 'sky', folders: [] },
  ],
  built_styles: ['plain'],
  known_styles: ['ultrasharp', 'remacri', 'siax', 'generalv3', 'x4plus', 'plain', 'blend'],
  default_style: 'ultrasharp',
  enabled_cvar: 'dodstudio_hd_enabled',
  style_cvar: 'dodstudio_hd_style',
  tools: {
    dir: 'C:/Users/me/AppData/Roaming/dod-studio/hd_tools/realesrgan',
    upscaler: 'C:/Users/me/AppData/Roaming/dod-studio/hd_tools/realesrgan/realesrgan-ncnn-vulkan.exe',
    upscaler_present: false,
    models: [{ style: 'ultrasharp', model: 'ultrasharp-4x', present: false }],
  },
};

async function loadHarness(page, handlers) {
  await page.addInitScript((h) => {
    window.__mockInvokeHandlers = window.__mockInvokeHandlers || {};
    // Functions can't cross addInitScript, so handlers are described by data.
    if (h.status) window.__mockInvokeHandlers.hd_status = () => h.status;
    if (h.statusError) window.__mockInvokeHandlers.hd_status = () => Promise.reject(h.statusError);
    window.__mockInvokeHandlers.hd_setup_tools = () =>
      new Promise((resolve, reject) => { window.__finishSetup = { resolve, reject }; });
  }, handlers);
  await page.goto('/tests/e2e/hd-textures.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

test('a status report fills the table, picks the built style and writes the cfg lines', async ({ page }) => {
  await loadHarness(page, { status: STATUS });
  await page.click('#hd-refresh-btn');

  const rows = page.locator('#hd-status-body tr');
  await expect(rows).toHaveCount(5);
  await expect(rows.nth(0)).toContainText('Map textures');
  await expect(rows.nth(0)).toContainText('plain: 4,708 files, 3.0 GB');
  // An empty overrides folder is not listed.
  await expect(rows.nth(0)).not.toContainText('overrides');
  await expect(rows.nth(1)).toContainText('Nothing yet');

  // The default isn't built, so the built one is picked.
  await expect(page.locator('#hd-style-select')).toHaveValue('plain');
  await expect(page.locator('#hd-cfg-lines')).toHaveText('dodstudio_hd_enabled 1\ndodstudio_hd_style plain');
  await expect(page.locator('#hd-style-select option[value="ultrasharp"]')).toContainText('(default) (not built yet)');

  await page.selectOption('#hd-style-select', 'remacri');
  await expect(page.locator('#hd-cfg-lines')).toHaveText('dodstudio_hd_enabled 1\ndodstudio_hd_style remacri');

  await expect(page.locator('#hd-tools-text')).toContainText('not downloaded yet');
  await expect(page.locator('#hd-realesrgan-line')).toHaveText(`set REALESRGAN=${STATUS.tools.upscaler}`);
});

test('opening the page refreshes it', async ({ page }) => {
  await loadHarness(page, { status: STATUS });
  await page.click('.nav-tab-btn[data-nav="hd-textures"]');
  await expect(page.locator('#hd-status-body tr')).toHaveCount(5);
});

test('no game path shows the backend message instead of a table', async ({ page }) => {
  const message = 'Set the Half-Life Executable in Configuration → Paths first.';
  await loadHarness(page, { statusError: message });
  await page.click('#hd-refresh-btn');
  await expect(page.locator('#hd-status-text')).toHaveText(message);
  await expect(page.locator('#hd-status-body tr')).toHaveCount(0);
});

test('download progress, then cancel', async ({ page }) => {
  await loadHarness(page, { status: STATUS });
  await page.click('#hd-setup-btn');
  await expect(page.locator('#hd-setup-btn')).toBeDisabled();
  await expect(page.locator('#hd-setup-cancel-btn')).toBeEnabled();

  await page.evaluate(() => window.__mockEmit('hd_setup_progress', {
    item: 'realesrgan-ncnn-vulkan.zip', step: 1, steps: 9,
    bytes_done: 10 * 1024 ** 2, bytes_total: 45 * 1024 ** 2, unpacking: false,
  }));
  await expect(page.locator('#hd-setup-progress')).toHaveText('1 of 9: realesrgan-ncnn-vulkan.zip (10.0 MB of 45.0 MB)');

  await page.click('#hd-setup-cancel-btn');
  const cancelled = await page.evaluate(() => window.__mockInvocations.some((c) => c.cmd === 'hd_setup_cancel'));
  expect(cancelled).toBe(true);
  await page.evaluate(() => window.__finishSetup.reject('cancelled'));
  await expect(page.locator('#hd-setup-progress')).toContainText('Cancelled');
  await expect(page.locator('#hd-setup-btn')).toBeEnabled();
});
