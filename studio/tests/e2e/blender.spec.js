// blender_pane.js — the Blender page (#403).
//
// Pins what the page shows for a status report (which Blender, its add-ons,
// the maps and HD styles to pick), that nothing can run until a take and the
// Crowbar folder are chosen, the requests each step sends, render progress,
// and cancel.
import { test, expect } from '@playwright/test';

/** blender_manager::BlenderPageStatus, as serde sends it (snake_case). */
const STATUS = {
  blender: {
    exe: 'C:/Programs/Blender 4.4.3/blender.exe',
    chosen: true,
    version: '4.4.3',
    supported: true,
    addons: [['io_scene_valvesource', true], ['advancedfx', false]],
    scripts: 'C:/dod-studio/blender',
  },
  maps: ['dod_anzio', 'dod_avalanche'],
  styles: ['ultrasharp'],
};

async function loadHarness(page, handlers) {
  await page.addInitScript((h) => {
    try { localStorage.clear(); } catch { /* ignore */ }
    window.__mockInvokeHandlers = window.__mockInvokeHandlers || {};
    // Functions can't cross addInitScript, so handlers are described by data.
    if (h.status) window.__mockInvokeHandlers.blender_status = () => h.status;
    window.__mockInvokeHandlers.blender_run = () =>
      new Promise((resolve, reject) => { window.__finishRun = { resolve, reject }; });
    window.__picks = h.picks || [];
    window.__mockInvokeHandlers['plugin:dialog|open'] = () => window.__picks.shift() ?? null;
  }, handlers);
  await page.goto('/tests/e2e/blender.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const lastRun = (page) => page.evaluate(() =>
  window.__mockInvocations.filter((c) => c.cmd === 'blender_run').at(-1)?.args);

test('opening the page shows the Blender it would use, its add-ons, and the maps and styles', async ({ page }) => {
  await loadHarness(page, { status: STATUS });
  await page.click('.nav-tab-btn[data-nav="blender"]');

  await expect(page.locator('#blender-status-text')).toHaveText(
    'Blender 4.4.3, the one you chose (C:/Programs/Blender 4.4.3/blender.exe).');
  await expect(page.locator('#blender-addons-text')).toContainText('not installed for this Blender: advancedfx');
  await expect(page.locator('#blender-map-select option')).toHaveText(
    ['(choose the map it was recorded on)', 'dod_anzio', 'dod_avalanche']);
  await expect(page.locator('#blender-style-select option')).toHaveText(['Original', 'ultrasharp']);
  // The first built style is the default.
  await expect(page.locator('#blender-style-select')).toHaveValue('ultrasharp');
  // No take yet: nothing runs.
  await expect(page.locator('#blender-import-btn')).toBeDisabled();
  await expect(page.locator('#blender-render-btn')).toBeDisabled();
});

test('another Blender version is flagged', async ({ page }) => {
  const other = { ...STATUS, blender: { ...STATUS.blender, version: '5.0.1', supported: false, chosen: false } };
  await loadHarness(page, { status: other });
  await page.click('#blender-refresh-btn');
  await expect(page.locator('#blender-status-text')).toContainText('need Blender 4.4');
});

test('each step sends its own request, for the chosen take, map and style', async ({ page }) => {
  await loadHarness(page, { status: STATUS, picks: ['D:/takes/streak.agr', 'D:/crowbar'] });
  await page.click('#blender-refresh-btn');
  await page.click('#blender-pick-agr-btn');
  await page.click('#blender-pick-assets-btn');
  await expect(page.locator('#blender-work-text')).toHaveText('Work folder: D:/takes\\streak_blender');

  // Build scene waits for a map.
  await expect(page.locator('#blender-scene-btn')).toBeDisabled();
  await expect(page.locator('#blender-import-btn')).toBeEnabled();

  await page.click('#blender-import-btn');
  expect(await lastRun(page)).toEqual({
    gamePath: 'C:/games/Half-Life/hl.exe',
    request: { agr: 'D:/takes/streak.agr', assets: 'D:/crowbar', map: '', style: 'ultrasharp', step: { kind: 'import' } },
  });
  // One step at a time.
  await expect(page.locator('#blender-render-btn')).toBeDisabled();
  await expect(page.locator('#blender-cancel-btn')).toBeEnabled();
  await page.evaluate(() => window.__finishRun.resolve({
    saved: ['D:/takes/streak_blender/imported.blend'], images: [], elapsed_secs: 12, work_dir: 'D:/takes/streak_blender',
  }));
  await expect(page.locator('#blender-progress')).toHaveText('Import: done in 12s.');
  await expect(page.locator('#blender-results li')).toHaveText(['D:/takes/streak_blender/imported.blend']);

  await page.selectOption('#blender-map-select', 'dod_anzio');
  await page.selectOption('#blender-style-select', 'none');
  await page.selectOption('#blender-engine-select', 'cycles');
  await page.fill('#blender-frames-input', '180, 510');
  await page.click('#blender-scene-btn');
  expect((await lastRun(page)).request).toEqual({
    agr: 'D:/takes/streak.agr', assets: 'D:/crowbar', map: 'dod_anzio', style: 'none',
    step: { kind: 'scene', engine: 'cycles', frames: [180, 510] },
  });
  await page.evaluate(() => window.__finishRun.resolve({ saved: [], images: [], elapsed_secs: 5, work_dir: '' }));

  await page.selectOption('#blender-quality-select', 'final');
  await page.click('#blender-render-btn');
  expect((await lastRun(page)).request.step).toEqual({ kind: 'render', quick: false });
  await page.evaluate(() => window.__finishRun.resolve({ saved: [], images: [], elapsed_secs: 5, work_dir: '' }));

  await page.click('#blender-encode-btn');
  expect((await lastRun(page)).request.step).toEqual({ kind: 'encode', quick: false });
});

test('render progress, then cancel', async ({ page }) => {
  await loadHarness(page, { status: STATUS, picks: ['D:/takes/streak.agr'] });
  await page.click('#blender-refresh-btn');
  await page.click('#blender-pick-agr-btn');
  await page.click('#blender-render-btn');

  await page.evaluate(() => window.__mockEmit('blender_progress', {
    line: 'frame 181 (2/977) 1.9s', done: 2, total: 977, frame_secs: 1.9, elapsed_secs: 65,
  }));
  await expect(page.locator('#blender-progress')).toHaveText('Frame 2 of 977 (1.9 s each), 1m 05s so far');
  await expect(page.locator('#blender-line')).toHaveText('frame 181 (2/977) 1.9s');

  await page.click('#blender-cancel-btn');
  const cancelled = await page.evaluate(() => window.__mockInvocations.some((c) => c.cmd === 'blender_cancel'));
  expect(cancelled).toBe(true);
  await page.evaluate(() => window.__finishRun.reject('cancelled'));
  await expect(page.locator('#blender-progress')).toContainText('Stopped');
  await expect(page.locator('#blender-render-btn')).toBeEnabled();
});

test('no game path says so instead of asking the backend', async ({ page }) => {
  await loadHarness(page, { status: STATUS });
  await page.fill('#hl-path-input', '');
  await page.click('#blender-refresh-btn');
  await expect(page.locator('#blender-status-text')).toContainText('Set the Half-Life Executable');
  const asked = await page.evaluate(() => window.__mockInvocations.some((c) => c.cmd === 'blender_status'));
  expect(asked).toBe(false);
});
