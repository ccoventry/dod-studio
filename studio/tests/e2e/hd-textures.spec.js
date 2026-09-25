// hd_pane.js — the HD Textures page (#372).
//
// Pins what the page shows for a status report (the per-type table, the style
// picker and the movie.cfg lines built from the backend's cvar names), that a
// missing game path reads as a message rather than a broken page, that the
// download's progress and cancel land in the progress line, which Python the
// page says it will use, and the build's choices, progress and cancel.
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
  python: {
    using: { source: 'found', exe: 'C:/Python314/python.exe', version: '3.14.7', missing: [] },
    chosen: null,
    chosen_problem: null,
    found_unusable: null,
    app_copy_present: false,
  },
  scripts: 'C:/dod-studio/goldsrc-hooks/tools/hd',
};

async function loadHarness(page, handlers) {
  await page.addInitScript((h) => {
    window.__mockInvokeHandlers = window.__mockInvokeHandlers || {};
    // Functions can't cross addInitScript, so handlers are described by data.
    if (h.status) window.__mockInvokeHandlers.hd_status = () => h.status;
    if (h.statusError) window.__mockInvokeHandlers.hd_status = () => Promise.reject(h.statusError);
    window.__mockInvokeHandlers.hd_setup_tools = () =>
      new Promise((resolve, reject) => { window.__finishSetup = { resolve, reject }; });
    window.__mockInvokeHandlers.hd_build = () =>
      new Promise((resolve, reject) => { window.__finishBuild = { resolve, reject }; });
    if (h.picked !== undefined) window.__mockInvokeHandlers['plugin:dialog|open'] = () => h.picked;
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
  const cancelled = await page.evaluate(() => window.__mockInvocations.some((c) => c.cmd === 'hd_cancel'));
  expect(cancelled).toBe(true);
  await page.evaluate(() => window.__finishSetup.reject('cancelled'));
  await expect(page.locator('#hd-setup-progress')).toContainText('Cancelled');
  await expect(page.locator('#hd-setup-btn')).toBeEnabled();
});

test('the Python line says which Python the build uses, and why another is not', async ({ page }) => {
  await loadHarness(page, { status: STATUS });
  await page.click('#hd-refresh-btn');
  await expect(page.locator('#hd-python-text')).toHaveText('Python: 3.14.7, found on this PC (C:/Python314/python.exe).');
  // Nothing chosen: nothing to reset.
  await expect(page.locator('#hd-python-reset-btn')).toBeHidden();

  const noneUsable = {
    ...STATUS,
    python: {
      using: null,
      chosen: 'D:/py/python.exe',
      chosen_problem: { exe: 'D:/py/python.exe', version: '3.13.1', missing: ['scipy'] },
      found_unusable: { exe: 'C:/Python39/python.exe', version: '3.9.18', missing: [] },
      app_copy_present: false,
    },
  };
  await page.evaluate((s) => { window.__mockInvokeHandlers.hd_status = () => s; }, noneUsable);
  await page.click('#hd-refresh-btn');
  const text = page.locator('#hd-python-text');
  await expect(text).toContainText('The Python you chose (D:/py/python.exe, 3.13.1) has no scipy');
  await expect(text).toContainText('Python 3.9.18 is installed (C:/Python39/python.exe) but is older than 3.10');
  await expect(page.locator('#hd-python-reset-btn')).toBeVisible();
  // No Python: no building.
  await expect(page.locator('#hd-build-btn')).toBeDisabled();
});

test('choosing a python.exe sends it to the backend, and reset forgets it', async ({ page }) => {
  await loadHarness(page, { status: STATUS, picked: 'D:/py/python.exe' });
  await page.click('#hd-refresh-btn');
  await page.click('#hd-python-pick-btn');
  await expect.poll(() => page.evaluate(() =>
    window.__mockInvocations.find((c) => c.cmd === 'hd_set_python')?.args)).toEqual({ path: 'D:/py/python.exe' });

  await page.evaluate((s) => {
    window.__mockInvokeHandlers.hd_status = () => ({ ...s, python: { ...s.python, chosen: 'D:/py/python.exe' } });
  }, STATUS);
  await page.click('#hd-refresh-btn');
  await page.click('#hd-python-reset-btn');
  await expect.poll(() => page.evaluate(() =>
    window.__mockInvocations.filter((c) => c.cmd === 'hd_set_python').at(-1)?.args)).toEqual({ path: null });
});

test('build choices: AI styles wait for the upscaler, and the request carries what is ticked', async ({ page }) => {
  await loadHarness(page, { status: STATUS });
  await page.click('#hd-refresh-btn');

  const styles = page.locator('#hd-build-styles input');
  await expect(styles).toHaveCount(7);
  // No upscaler yet: every AI style is off and says why; plain and blend are free.
  await expect(page.locator('#hd-build-styles input[value="ultrasharp"]')).toBeDisabled();
  await expect(page.locator('#hd-build-styles label', { hasText: 'x4plus' })).toContainText('(needs Download)');
  await expect(page.locator('#hd-build-styles input[value="plain"]')).toBeEnabled();
  // Every file type starts ticked.
  await expect(page.locator('#hd-build-types input:checked')).toHaveCount(5);

  await page.check('#hd-build-styles input[value="plain"]');
  await page.uncheck('#hd-build-types input[value="world"]');
  await page.click('#hd-build-btn');
  const request = await page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'hd_build')?.args);
  expect(request).toEqual({
    gamePath: 'C:/games/Half-Life/hl.exe',
    request: { styles: ['plain'], types: ['sky', 'sprites', 'models', 'detail'] },
  });
});

test('build progress, the finished line, and cancel', async ({ page }) => {
  await loadHarness(page, { status: STATUS });
  await page.click('#hd-refresh-btn');
  await page.check('#hd-build-styles input[value="plain"]');
  await page.click('#hd-build-btn');
  await expect(page.locator('#hd-build-btn')).toBeDisabled();
  await expect(page.locator('#hd-setup-btn')).toBeDisabled();

  await page.evaluate(() => window.__mockEmit('hd_build_progress', {
    step: 5, steps: 10, style: 'plain', asset_type: 'world', line: null, elapsed_secs: 75,
  }));
  await expect(page.locator('#hd-build-progress')).toHaveText('Step 5 of 10: plain, Map textures (1m 15s so far)');
  await page.evaluate(() => window.__mockEmit('hd_build_progress', {
    step: 5, steps: 10, style: 'plain', asset_type: 'world', line: 'plain      world   exit 0, 12 new', elapsed_secs: 80,
  }));
  await expect(page.locator('#hd-build-line')).toHaveText('plain      world   exit 0, 12 new');

  await page.click('#hd-build-cancel-btn');
  await expect.poll(() => page.evaluate(() => window.__mockInvocations.some((c) => c.cmd === 'hd_cancel'))).toBe(true);
  await page.evaluate(() => window.__finishBuild.reject('cancelled'));
  await expect(page.locator('#hd-build-progress')).toContainText('Stopped. Files already built are kept');
  await expect(page.locator('#hd-build-btn')).toBeEnabled();

  await page.click('#hd-build-btn');
  await page.evaluate(() => window.__finishBuild.resolve({ steps: 10, elapsed_secs: 200, log_path: 'C:/x/build_all.log' }));
  await expect(page.locator('#hd-build-progress')).toHaveText("Done: 10 steps in 3m 20s. Every step's counts are in C:/x/build_all.log.");
});
