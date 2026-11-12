// Project settings › Clip settings (#1085, build plan step 8) in the real app
// shell: a preset copies its lines in, a row rewrites its line, a row setting
// typed into Your own commands moves into its row, field of view lives on
// the Project tab, never unset, and what a batch would send follows the
// project: its field of view goes as picture_fov, sent after the lines
// (#1085), and a mirv_fov line shows in Your own commands as refused.
import { test, expect } from '@playwright/test';

const GIB = 1024 ** 3;
const CLEAN = ['r_decals 256', 'mirv_fov 90', 'ds_hud_hide_scoreboard 1', 'ds_spec_match_pov 1', 'mirv_movie_sound_volume 0.25'];

test.beforeEach(async ({ page }) => {
  await page.addInitScript(({ gib, clean }) => {
    window.__mockInvokeHandlers = {
      get_settings: () => ({
        setup_dismissed: true,
        // The page shows only with the New UI switch on (new_ui.js).
        new_ui: true,
        target_drives: ['E:/caps'],
        render_export_dirs: ['F:/clips'],
        resolution_width: 1920,
        resolution_height: 1080,
        capture_fps: 120,
        pc_id: 'pc-test',
        hlae_path: 'C:/HLAE/HLAE.exe',
        hl_path: 'C:/Games/Half-Life/hl.exe',
        init_commands: ['cl_righthand 1'],
        custom_commands: [{ command: 'cl_righthand 0', relation: 'Before', offset_seconds: 1 }],
      }),
      builtin_command_profiles: () => [{ name: 'Clean clips', init_commands: clean, custom_commands: [] }],
      calculate_export_pool_space: () => 10 * gib,
      system_memory_bytes: () => 16 * gib,
    };
  }, { gib: GIB, clean: CLEAN });
  await page.goto('/index.html');
  await expect(page.locator('#export-pool-free-indicator')).toContainText('10.0');
});

const pageRoot = (page) => page.locator('#project-settings-page');
const chip = (page, group, value) => pageRoot(page).locator(`[data-chips="${group}"] [data-value="${value}"]`);
const footer = (page, action) => pageRoot(page).locator(`.ps-footer [data-action="${action}"]`);
const tab = (page, id) => pageRoot(page).locator(`.ps-tab[data-tab="${id}"]`);

async function openClipTab(page) {
  await page.locator('#file-menu-btn').click();
  await page.locator('#project-settings-menu-btn').click();
  await expect(pageRoot(page)).toBeVisible();
  await tab(page, 'clip').click();
  await expect(tab(page, 'clip')).toHaveClass(/ps-tab-on/);
}

/** The Initial and Scheduled Commands Start Capture Batch would send now. */
const batchCommands = (page) => page.evaluate(async () => {
  const { batchSettingsPayload } = await import('/src/capture_pane.js');
  const payload = batchSettingsPayload({ targetDrives: ['E:/caps'] });
  return { init: payload.init_commands, custom: payload.custom_commands, fov: payload.picture_fov };
});

test('the tab lists the rows, the presets, and the lines no row shows', async ({ page }) => {
  await openClipTab(page);
  await expect(chip(page, 'preset', 'Clean clips')).toBeVisible();
  await expect(chip(page, 'ds_hud_hide_scoreboard', 'unset')).toHaveAttribute('aria-pressed', 'true');
  await expect(pageRoot(page).locator('[data-field^="own-"]')).toHaveValue('cl_righthand 1');
  await expect(pageRoot(page).locator('[data-row="clip-hd_textures"]')).toContainText('At map load');
  await expect(pageRoot(page).locator('[data-field="scheduled-0"]')).toHaveValue('cl_righthand 0');
});

test('picking Clean clips copies its lines in; Apply makes them what a batch sends', async ({ page }) => {
  const before = await batchCommands(page);
  expect(before.init).toEqual(['cl_righthand 1']);
  await openClipTab(page);
  await chip(page, 'preset', 'Clean clips').click();
  await expect(chip(page, 'ds_hud_hide_scoreboard', '1')).toHaveAttribute('aria-pressed', 'true');
  await expect(pageRoot(page).locator('[data-field="mirv_movie_sound_volume"]')).toHaveValue('0.25');
  await footer(page, 'apply').click();
  await expect(page.locator('#session-file-indicator')).toContainText('unsaved');
  await footer(page, 'ok').click();
  expect((await batchCommands(page)).init).toEqual(CLEAN);
});

test('changing a row marks the preset edited and rewrites only that line', async ({ page }) => {
  await openClipTab(page);
  await chip(page, 'preset', 'Clean clips').click();
  await chip(page, 'ds_hud_hide_scoreboard', '0').click();
  await expect(chip(page, 'preset', 'Clean clips')).toHaveText('Clean clips (edited)');
  await footer(page, 'ok').click();
  const sent = (await batchCommands(page)).init;
  expect(sent).toEqual(CLEAN.map((line) => (line === 'ds_hud_hide_scoreboard 1' ? 'ds_hud_hide_scoreboard 0' : line)));
});

test('a row setting typed into Your own commands moves into its row', async ({ page }) => {
  await openClipTab(page);
  await pageRoot(page).getByRole('button', { name: '+ Add a command' }).first().click();
  const box = pageRoot(page).locator('[data-field^="own-"]').last();
  await box.fill('ds_spec_xray_show 2');
  await box.press('Tab');
  await expect(pageRoot(page).locator('[data-field="ds_spec_xray_show"]')).toHaveValue('2');
  await expect(pageRoot(page).locator('[data-field^="own-"]')).toHaveCount(1);
});

test('field of view is set on the Project tab, never unset, and goes after the lines', async ({ page }) => {
  await openClipTab(page);
  await chip(page, 'preset', 'Clean clips').click();
  // Clean clips' own mirv_fov line is no row's: it shows as refused.
  const own = pageRoot(page).locator('.ps-command', { has: page.locator('[data-field^="own-"]') });
  await expect(own.locator('[data-field^="own-"]')).toHaveValue('mirv_fov 90');
  await expect(own.locator('.ps-warn')).toContainText('Project Settings › Picture');
  await expect(pageRoot(page).locator('[data-row="clip-fov"]')).toContainText('90°');
  await pageRoot(page).locator('[data-row="clip-fov"]').getByRole('button', { name: 'Set on the Project tab' }).click();
  await expect(tab(page, 'project')).toHaveClass(/ps-tab-on/);
  // Never "not set": 90, the game's own, picked, and no way to clear it.
  await expect(chip(page, 'fov', '90')).toHaveAttribute('aria-pressed', 'true');
  await expect(pageRoot(page).locator('[data-row="capture_fov"]')).not.toContainText('Not set');
  await footer(page, 'apply').click();
  expect(await batchCommands(page)).toMatchObject({ init: CLEAN, fov: 90 });

  await chip(page, 'fov', '100').click();
  await footer(page, 'ok').click();
  // The lines go as they are: the backend sends 100 after them, leaving
  // their own mirv_fov out (native picture_fov.rs).
  expect(await batchCommands(page)).toMatchObject({ init: CLEAN, fov: 100 });
});

test('a mirv_fov number typed in Your own commands is refused; handleZoom is not', async ({ page }) => {
  await openClipTab(page);
  const add = pageRoot(page).getByRole('button', { name: '+ Add a command' }).first();
  await add.click();
  const line = pageRoot(page).locator('.ps-command', { has: page.locator('[data-field^="own-"]') }).last();
  const box = line.locator('[data-field^="own-"]');
  const note = line.locator('.ps-warn');
  await box.fill('mirv_fov 105');
  await expect(note).toContainText('Refused');
  await box.fill('mirv_fov handleZoom enabled 1');
  await expect(note).toBeHidden();
});

test('Save as new puts the rows in a preset on this PC and picks it', async ({ page }) => {
  await openClipTab(page);
  await chip(page, 'ds_hud_show_scope', '1').click();
  await pageRoot(page).getByRole('button', { name: '+ Save as new…' }).click();
  await pageRoot(page).locator('[data-field="preset-name"]').fill('FOTW');
  await pageRoot(page).locator('[data-action="save-preset"]').click();
  await expect(chip(page, 'preset', 'FOTW')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('#command-profile-select option[value="FOTW"]')).toHaveCount(1);
});

test('an At each clip command with no seconds greys Apply and says why', async ({ page }) => {
  await openClipTab(page);
  await pageRoot(page).locator('[data-field="scheduled-seconds-0"]').fill('');
  await expect(footer(page, 'apply')).toBeDisabled();
  await expect(pageRoot(page).locator('[data-row="custom_commands"] .ps-problem')).toBeVisible();
});
