// hd_pane.js — the HD Textures page (#372).
//
// Pins what the page shows for a status report (the style-by-type table, the style
// picker and the movie.cfg lines built from the backend's cvar names), that a
// missing game path reads as a message rather than a broken page, that the
// download's progress and cancel land in the progress line, which Python the
// page says it will use, the build's choices, progress and cancel, and the
// style comparison, the custom-style form (my_styles.txt), and the misses
// view read from the hook log.
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
    source: null,
    chosen: null,
    models: [
      { style: 'ultrasharp', model: 'ultrasharp-4x', present: false },
      { style: 'remacri', model: 'remacri-4x', present: false },
      { style: 'siax', model: '4x_NMKD-Siax_200k', present: false },
      { style: 'generalv3', model: 'RealESRGAN_General_x4_v3', present: false },
      { style: 'x4plus', model: 'realesrgan-x4plus', present: false },
    ],
    available_models: [],
  },
  python: {
    using: { source: 'found', exe: 'C:/Python314/python.exe', version: '3.14.7', missing: [] },
    chosen: null,
    chosen_problem: null,
    found_unusable: null,
    app_copy_present: false,
  },
  scripts: 'C:/dod-studio/goldsrc-hooks/tools/hd',
  my_styles: {
    path: 'C:/games/Half-Life/dod/dodstudio_hd/my_styles.txt',
    exists: false,
    old_place: null,
    styles: [],
    error: null,
  },
  maps: ['dod_anzio', 'dod_caen'],
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
    if (h.misses !== undefined) window.__mockInvokeHandlers.hd_misses = () => h.misses;
    if (h.saveError) window.__mockInvokeHandlers.hd_save_style = () => Promise.reject(h.saveError);
    window.__mockInvokeHandlers.hd_preview = () =>
      new Promise((resolve, reject) => { window.__finishPreview = { resolve, reject }; });
  }, handlers);
  await page.goto('/tests/e2e/hd-textures.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

test('a status report fills the table, picks the built style and writes the cfg lines', async ({ page }) => {
  await loadHarness(page, { status: STATUS });
  await page.click('#hd-refresh-btn');

  // Styles are rows, asset types are columns.
  await expect(page.locator('#hd-status-head th')).toHaveText(
    ['Style', 'Map textures', 'Model skins', 'Sprites', 'Detail textures', 'Skies']);
  const rows = page.locator('#hd-status-body tr');
  // Only plain has files; an empty overrides folder is not a row.
  await expect(rows).toHaveCount(1);
  await expect(rows.nth(0).locator('td')).toHaveText(
    ['plain', '4,708 files, 3.0 GB', '–', '–', '493 files, 50.0 MB', '–']);

  // The default isn't built, so the built one is picked.
  await expect(page.locator('#hd-style-select')).toHaveValue('plain');
  await expect(page.locator('#hd-cfg-lines')).toHaveText('dodstudio_hd_enabled 1\ndodstudio_hd_style plain');
  await expect(page.locator('#hd-style-select option[value="ultrasharp"]')).toContainText('(default) (not built yet)');

  await page.selectOption('#hd-style-select', 'remacri');
  await expect(page.locator('#hd-cfg-lines')).toHaveText('dodstudio_hd_enabled 1\ndodstudio_hd_style remacri');

  await expect(page.locator('#hd-tools-text')).toContainText('Upscaler: none found');
  await expect(page.locator('#hd-realesrgan-line')).toHaveText(`set REALESRGAN=${STATUS.tools.upscaler}`);
});

test('opening the page refreshes it', async ({ page }) => {
  await loadHarness(page, { status: STATUS });
  await page.click('.nav-tab-btn[data-nav="hd-textures"]');
  await expect(page.locator('#hd-status-body tr')).toHaveCount(1);
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
      // PIL is reported as Pillow, the name pip knows.
      found_unusable: { exe: 'C:/Python39/python.exe', version: '3.9.18', missing: [] },
      app_copy_present: false,
    },
  };
  await page.evaluate((s) => { window.__mockInvokeHandlers.hd_status = () => s; }, noneUsable);
  await page.click('#hd-refresh-btn');
  const text = page.locator('#hd-python-text');
  await expect(text).toContainText('The Python you chose (D:/py/python.exe, 3.13.1) has no SciPy');
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

test('rows follow the style order, custom styles after, overrides last; nothing built is one row', async ({ page }) => {
  const types = ['world', 'models', 'sprites', 'detail', 'sky'];
  const withFolders = {
    ...STATUS,
    built_styles: ['crisp', 'plain', 'ultrasharp'],
    types: types.map((asset_type) => ({
      asset_type,
      folders: asset_type === 'world'
        ? ['crisp', 'overrides', 'plain', 'ultrasharp'].map((name) => ({ name, files: 1, bytes: 1024 }))
        : [],
    })),
  };
  await loadHarness(page, { status: withFolders });
  await page.click('#hd-refresh-btn');
  await expect(page.locator('#hd-status-body tr td:first-child')).toHaveText(['ultrasharp', 'plain', 'crisp', 'overrides']);

  await page.evaluate((s) => {
    window.__mockInvokeHandlers.hd_status = () => ({ ...s, built_styles: [], types: s.types.map((t) => ({ ...t, folders: [] })) });
  }, STATUS);
  await page.click('#hd-refresh-btn');
  await expect(page.locator('#hd-status-body tr')).toHaveCount(1);
  await expect(page.locator('#hd-status-body td')).toHaveText('Nothing yet');
  await expect(page.locator('#hd-status-body td')).toHaveAttribute('colspan', '6');
});

test('an upscaler found elsewhere is named and its styles can build; a chosen folder can be reset', async ({ page }) => {
  const dir = 'C:/dod-studio/local/texture-upscale-rnd/tools/realesrgan';
  const found = {
    ...STATUS,
    tools: {
      ...STATUS.tools,
      dir,
      upscaler: `${dir}/realesrgan-ncnn-vulkan.exe`,
      upscaler_present: true,
      source: 'chosen',
      chosen: dir,
      models: STATUS.tools.models.map((m) => ({ ...m, present: m.style !== 'siax' })),
    },
  };
  await loadHarness(page, { status: found, picked: dir });
  await page.click('#hd-refresh-btn');
  const text = page.locator('#hd-tools-text');
  await expect(text).toContainText(`Upscaler: the folder you chose (${dir}).`);
  await expect(text).toContainText('Missing models for: siax.');
  await expect(page.locator('#hd-build-styles input[value="ultrasharp"]')).toBeEnabled();
  await expect(page.locator('#hd-build-styles input[value="siax"]')).toBeDisabled();
  await expect(page.locator('#hd-upscaler-reset-btn')).toBeVisible();

  await page.click('#hd-upscaler-pick-btn');
  await expect.poll(() => page.evaluate(() =>
    window.__mockInvocations.find((c) => c.cmd === 'hd_set_upscaler')?.args)).toEqual({ path: dir });
  const picker = await page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'plugin:dialog|open')?.args);
  expect(picker.options.directory).toBe(true);

  await page.click('#hd-upscaler-reset-btn');
  await expect.poll(() => page.evaluate(() =>
    window.__mockInvocations.filter((c) => c.cmd === 'hd_set_upscaler').at(-1)?.args)).toEqual({ path: null });

  // A chosen folder another one beats is said so.
  await page.evaluate((s) => {
    window.__mockInvokeHandlers.hd_status = () => ({ ...s, tools: { ...s.tools, source: 'app' } });
  }, found);
  await page.click('#hd-refresh-btn');
  await expect(text).toContainText("isn't used: another one has more of the style models");
});

test('refresh shows it is working, and the status line says when it last finished', async ({ page }) => {
  await loadHarness(page, { status: STATUS });
  await page.evaluate((s) => {
    window.__mockInvokeHandlers.hd_status = () => new Promise((resolve) => { window.__finishStatus = () => resolve(s); });
  }, STATUS);
  await page.click('#hd-refresh-btn');
  await expect(page.locator('#hd-refresh-btn')).toBeDisabled();
  await expect(page.locator('#hd-refresh-btn')).toHaveText('Checking...');
  await page.evaluate(() => window.__finishStatus());
  await expect(page.locator('#hd-refresh-btn')).toBeEnabled();
  await expect(page.locator('#hd-refresh-btn')).toHaveText('Refresh');
  await expect(page.locator('#hd-status-text')).toContainText(/Checked at .+\./);
});

test('missing packages are named as pip knows them', async ({ page }) => {
  const status = {
    ...STATUS,
    python: {
      using: null, chosen: null, chosen_problem: null, app_copy_present: false,
      found_unusable: { exe: 'C:/Py/python.exe', version: '3.12.1', missing: ['numpy', 'PIL'] },
    },
  };
  await loadHarness(page, { status });
  await page.click('#hd-refresh-btn');
  await expect(page.locator('#hd-python-text')).toContainText('has no NumPy, Pillow.');
});

test('Download is off, and says so, when there is nothing to download', async ({ page }) => {
  const complete = {
    ...STATUS,
    tools: { ...STATUS.tools, upscaler_present: true, source: 'app',
      models: STATUS.tools.models.map((m) => ({ ...m, present: true })) },
  };
  await loadHarness(page, { status: complete });
  await page.click('#hd-refresh-btn');
  await expect(page.locator('#hd-setup-btn')).toBeDisabled();
  await expect(page.locator('#hd-setup-btn')).toHaveText('Nothing to download');

  // One model short: back on.
  await page.evaluate((s) => {
    window.__mockInvokeHandlers.hd_status = () => ({
      ...s, tools: { ...s.tools, models: s.tools.models.map((m, i) => ({ ...m, present: i > 0 })) },
    });
  }, complete);
  await page.click('#hd-refresh-btn');
  await expect(page.locator('#hd-setup-btn')).toBeEnabled();
  await expect(page.locator('#hd-setup-btn')).toHaveText("Download what's missing");
});

test('Build is off until at least one style and one kind of file are ticked', async ({ page }) => {
  await loadHarness(page, { status: STATUS });
  await page.click('#hd-refresh-btn');
  // The default style (ultrasharp) needs the upscaler, so no style starts ticked.
  await expect(page.locator('#hd-build-styles input:checked')).toHaveCount(0);
  await expect(page.locator('#hd-build-btn')).toBeDisabled();

  await page.check('#hd-build-styles input[value="plain"]');
  await expect(page.locator('#hd-build-btn')).toBeEnabled();

  for (const type of ['sky', 'sprites', 'models', 'detail', 'world']) {
    await page.uncheck(`#hd-build-types input[value="${type}"]`);
  }
  await expect(page.locator('#hd-build-btn')).toBeDisabled();
  await page.check('#hd-build-types input[value="sky"]');
  await expect(page.locator('#hd-build-btn')).toBeEnabled();

  await page.uncheck('#hd-build-styles input[value="plain"]');
  await expect(page.locator('#hd-build-btn')).toBeDisabled();
});

/** native::hd::misses::MissesView, as serde sends it. */
const MISSES = {
  command: 'dodstudio_debug_hd_misses',
  report: {
    log_file: 'C:/Users/me/AppData/Roaming/dod-studio/logs/dodstudio_goldsrc_hooks_20260924.log',
    date: '2026-09-24',
    time: '22:05:57',
    summary: '4 miss(es) this session, 3 different texture(s) (style "plain"). A texture several maps use is listed under each of them.',
    style: 'plain',
    maps: [
      {
        map: 'dod_anzio', total: 3, on_purpose: 1,
        groups: [
          {
            reason: 'no_file', heading: 'no HD file',
            entries: [{ asset_type: 'model', name: 'models/v_garand.mdl garand.bmp', detail: '256x128, not built yet', loads: 2, also_on: [] }],
          },
          {
            reason: 'wrong_version', heading: 'HD file is for a different version of the texture',
            entries: [{ asset_type: 'world', name: 'bido_wall1', detail: '128x128', loads: 1, also_on: ['dod_caen', 'dod_flash'] }],
          },
          {
            reason: 'on_purpose', heading: 'left alone on purpose',
            entries: [{ asset_type: 'sprite', name: 'sprites/puff.spr', detail: 'blank', loads: 4, also_on: ['dod_caen'] }],
          },
        ],
      },
      {
        map: 'dod_caen', total: 1, on_purpose: 1,
        groups: [{
          reason: 'on_purpose', heading: 'left alone on purpose',
          entries: [{ asset_type: 'sprite', name: 'sprites/puff.spr', detail: 'blank', loads: 4, also_on: ['dod_anzio'] }],
        }],
      },
    ],
  },
};

test('misses: nothing in the log says how to get a list, and names the command', async ({ page }) => {
  await loadHarness(page, { status: STATUS, misses: { command: 'dodstudio_debug_hd_misses', report: null } });
  await page.click('.nav-tab-btn[data-nav="hd-textures"]');
  await expect(page.locator('#hd-misses-command')).toHaveText('dodstudio_debug_hd_misses');
  await expect(page.locator('#hd-misses-text')).toContainText('No list in the game');
  await expect(page.locator('#hd-misses-maps details')).toHaveCount(0);
});

test('misses: a list is shown map by map, on-purpose ones only when asked', async ({ page }) => {
  await loadHarness(page, { status: STATUS, misses: MISSES });
  await page.click('#hd-misses-btn');
  await expect(page.locator('#hd-misses-text')).toHaveText(
    `From 2026-09-24 at 22:05:57, style plain: ${MISSES.report.summary}`);

  const maps = page.locator('#hd-misses-maps details');
  await expect(maps).toHaveCount(2);
  await expect(maps.nth(0).locator('summary')).toHaveText('dod_anzio: 3 kept their original, 1 of them on purpose');
  // On-purpose groups are hidden by default; a map with nothing else says so.
  await expect(maps.nth(0).locator('.hd-miss-heading')).toHaveText(
    ['no HD file (1)', 'HD file is for a different version of the texture (1)']);
  await expect(maps.nth(1)).toContainText('Only textures left alone on purpose.');

  const skin = maps.nth(0).locator('li').nth(0);
  await expect(skin.locator('.hd-miss-type')).toHaveText('Model skin');
  await expect(skin).toContainText('models/v_garand.mdl garand.bmp');
  await expect(skin).toContainText('256x128, not built yet, 2 loads');
  const shared = maps.nth(0).locator('li').nth(1).locator('.hd-miss-detail').nth(1);
  await expect(shared).toHaveText('also on 2 other maps');
  await expect(shared).toHaveAttribute('title', 'dod_caen, dod_flash');

  await page.check('#hd-misses-on-purpose');
  await expect(maps.nth(0).locator('.hd-miss-heading')).toHaveCount(3);
  await expect(maps.nth(1).locator('li')).toContainText(['Sprite']);
  await expect(maps.nth(1).locator('li')).toContainText(['4 frames']);
});

/** STATUS with a my_styles.txt holding one style of each kind, and an
 *  upscaler folder that has the x4plus model and nothing else. */
const WITH_MY_STYLES = {
  ...STATUS,
  tools: {
    ...STATUS.tools,
    upscaler_present: true,
    source: 'app',
    models: STATUS.tools.models.map((m) => ({ ...m, present: m.style === 'x4plus' })),
    available_models: ['realesrgan-x4plus', 'realesrgan-x4plus-anime'],
  },
  my_styles: {
    ...STATUS.my_styles,
    exists: true,
    styles: [
      { name: 'crisp', kind: 'plain', sharpening: 150 },
      { name: 'anime', kind: 'ai', model: 'realesrgan-x4plus-anime' },
      { name: 'odd', kind: 'ai', model: 'not-downloaded' },
      { name: 'sharp70', kind: 'blend', a: 'ultrasharp', b: 'plain', percent: 70 },
    ],
  },
};

test('my styles: listed, offered to Build, and an AI one waits for its model', async ({ page }) => {
  await loadHarness(page, { status: WITH_MY_STYLES });
  await page.click('#hd-refresh-btn');
  await expect(page.locator('#hd-my-styles-text')).toHaveText(`Saved in ${STATUS.my_styles.path}.`);
  const items = page.locator('#hd-my-styles-list li');
  await expect(items).toHaveCount(4);
  await expect(items.nth(0)).toContainText('crisp');
  await expect(items.nth(0)).toContainText('plain enlargement, sharpening 150');
  await expect(items.nth(3)).toContainText('70% ultrasharp, 30% plain');

  // Each is a Build choice, after the built-in ones.
  const choice = (name) => page.locator(`#hd-build-styles input[value="${name}"]`);
  await expect(choice('crisp')).toBeEnabled();
  await expect(choice('anime')).toBeEnabled();
  await expect(choice('sharp70')).toBeEnabled();
  await expect(choice('odd')).toBeDisabled();
  await expect(page.locator('#hd-build-styles label').filter({ hasText: 'odd' })).toHaveText('odd (needs its model)');
  // The blend form offers every style, the user's own included.
  await expect(page.locator('#hd-style-a option')).toHaveCount(11);
});

test('my styles: the form shows its line, refuses bad names, and saves', async ({ page }) => {
  await loadHarness(page, { status: WITH_MY_STYLES });
  await page.click('#hd-refresh-btn');
  const save = page.locator('#hd-style-save-btn');
  await expect(save).toBeDisabled();

  await page.fill('#hd-style-name', 'Crisp2!');
  await expect(page.locator('#hd-style-message')).toContainText('lowercase letters');
  await expect(save).toBeDisabled();
  await page.fill('#hd-style-name', 'plain');
  await expect(page.locator('#hd-style-message')).toHaveText('plain is a built-in style; pick another name.');

  await page.fill('#hd-style-name', 'Soft');
  await expect(page.locator('#hd-style-message')).toHaveText('');
  await page.fill('#hd-style-sharpening', '20');
  await expect(page.locator('#hd-style-line')).toHaveText('soft = plain 20');

  await page.selectOption('#hd-style-kind', 'ai');
  await expect(page.locator('#hd-style-model')).toBeVisible();
  await page.selectOption('#hd-style-model', 'realesrgan-x4plus-anime');
  await expect(page.locator('#hd-style-line')).toHaveText('soft = realesrgan-x4plus-anime');

  await page.selectOption('#hd-style-kind', 'blend');
  await page.fill('#hd-style-percent', '30');
  await page.selectOption('#hd-style-a', 'crisp');
  await page.selectOption('#hd-style-b', 'x4plus');
  await expect(page.locator('#hd-style-line')).toHaveText('soft = blend crisp x4plus 30');

  await save.click();
  const call = await page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'hd_save_style'));
  expect(call.args).toEqual({
    gamePath: 'C:/games/Half-Life/hl.exe',
    name: 'soft',
    def: { kind: 'blend', a: 'crisp', b: 'x4plus', percent: 30 },
  });
  await expect(page.locator('#hd-style-message')).toContainText('Saved soft.');
});

test("my styles: a blend of itself is refused, and the backend's refusal is shown", async ({ page }) => {
  await loadHarness(page, { status: WITH_MY_STYLES, saveError: 'my_styles.txt: soft blends "nope", which isn\'t a style' });
  await page.click('#hd-refresh-btn');
  await page.fill('#hd-style-name', 'crisp');
  await page.selectOption('#hd-style-kind', 'blend');
  await page.selectOption('#hd-style-a', 'crisp');
  await expect(page.locator('#hd-style-message')).toHaveText("A style can't mix itself.");
  await expect(page.locator('#hd-style-save-btn')).toBeDisabled();

  await page.fill('#hd-style-name', 'soft');
  await page.click('#hd-style-save-btn');
  await expect(page.locator('#hd-style-message')).toContainText("which isn't a style");
});

test('my styles: Edit fills the form, Remove asks the backend', async ({ page }) => {
  await loadHarness(page, { status: WITH_MY_STYLES });
  await page.click('#hd-refresh-btn');
  const sharp70 = page.locator('#hd-my-styles-list li').nth(3);
  await sharp70.getByRole('button', { name: 'Edit' }).click();
  await expect(page.locator('#hd-style-name')).toHaveValue('sharp70');
  await expect(page.locator('#hd-style-kind')).toHaveValue('blend');
  await expect(page.locator('#hd-style-line')).toHaveText('sharp70 = blend ultrasharp plain 70');

  await page.locator('#hd-my-styles-list li').nth(0).getByRole('button', { name: 'Remove' }).click();
  const call = await page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'hd_remove_style'));
  expect(call.args).toEqual({ gamePath: 'C:/games/Half-Life/hl.exe', name: 'crisp' });
  await expect(page.locator('#hd-style-message')).toContainText('Removed crisp');
});

test('my styles: a file the scripts would refuse is reported, and Build waits for a fix', async ({ page }) => {
  const broken = { ...STATUS, my_styles: { ...STATUS.my_styles, exists: true, error: 'my_styles.txt line 3: expected `name = ...`, got "crisp plain"' } };
  await loadHarness(page, { status: broken });
  await page.click('#hd-refresh-btn');
  await expect(page.locator('#hd-my-styles-text')).toContainText("can't be read, so builds won't start");
  await expect(page.locator('#hd-build-btn')).toBeDisabled();
});

test('preview: picks maps and built styles, shows the sheet, and fits it on a click', async ({ page }) => {
  const built = { ...STATUS, built_styles: ['plain', 'ultrasharp'] };
  await loadHarness(page, { status: built });
  await page.click('#hd-refresh-btn');
  await expect(page.locator('#hd-preview-map option')).toHaveText(
    ['A few of your maps, picked for you', 'dod_anzio', 'dod_caen']);
  // Only built styles can be compared; all are ticked to start with.
  await expect(page.locator('#hd-preview-styles input:checked')).toHaveCount(2);

  await page.selectOption('#hd-preview-map', 'dod_caen');
  await page.uncheck('#hd-preview-styles input[value="ultrasharp"]');
  await page.click('#hd-preview-btn');
  await expect(page.locator('#hd-preview-btn')).toBeDisabled();
  const call = await page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'hd_preview'));
  expect(call.args).toEqual({
    gamePath: 'C:/games/Half-Life/hl.exe',
    request: { maps: ['dod_caen'], styles: ['plain'] },
  });

  // A 1x1 PNG stands in for the sheet.
  const png = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';
  await page.evaluate((image) => window.__finishPreview.resolve({
    image, samples: 3, maps: [], skipped: ['model:v_bar.mdl: not found'],
  }), png);
  await expect(page.locator('#hd-preview-wrap')).toBeVisible();
  await expect(page.locator('#hd-preview-img')).toHaveAttribute('src', png);
  await expect(page.locator('#hd-preview-text')).toHaveText('3 samples. Left out: model:v_bar.mdl: not found.');
  await expect(page.locator('#hd-preview-btn')).toBeEnabled();

  await page.click('#hd-preview-img');
  await expect(page.locator('#hd-preview-wrap')).toHaveClass(/hd-preview-fit/);
});

test('preview: auto-picked maps are named; nothing built means nothing to compare', async ({ page }) => {
  await loadHarness(page, { status: STATUS });
  await page.click('#hd-refresh-btn');
  await page.click('#hd-preview-btn');
  const call = await page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'hd_preview'));
  expect(call.args.request).toEqual({ maps: [], styles: ['plain'] });
  await page.evaluate(() => window.__finishPreview.resolve({
    image: 'data:image/png;base64,', samples: 9, maps: ['dod_anzio', 'dod_caen'], skipped: [],
  }));
  await expect(page.locator('#hd-preview-text')).toHaveText('9 samples, from dod_anzio, dod_caen.');

  await loadHarness(page, { status: { ...STATUS, built_styles: [] } });
  await page.click('#hd-refresh-btn');
  await expect(page.locator('#hd-preview-btn')).toBeDisabled();
  await expect(page.locator('#hd-preview-text')).toContainText('Build a style first');
});
