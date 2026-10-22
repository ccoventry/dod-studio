// command_profiles_ui.js — named sets of Initial and Scheduled Commands (#442).
import { test, expect } from '@playwright/test';

const FOTW = {
  init_commands: ['mirv_fov 90', 'r_decals 256'],
  custom_commands: [{ command: 'host_timescale 0.5', relation: 'Before', offset_seconds: 1.5 }],
};
const CLEAN = { init_commands: ['hud_draw 0'], custom_commands: [] };

async function loadHarness(page) {
  await page.goto('/tests/e2e/command-profiles.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const picked = (page) => page.locator('#command-profile-select option:checked');

test('saving names the current lists, and edits after it show "(edited)" until saved back', async ({ page }) => {
  await loadHarness(page);
  const select = page.locator('#command-profile-select');
  const saveChanges = page.locator('#command-profile-save-changes');
  await expect(select).toHaveValue('');
  await expect(page.locator('#command-profile-delete')).toBeDisabled();

  await page.evaluate((l) => window.__editLists(l), FOTW);
  await page.locator('#command-profile-name').fill('FOTW');
  await page.locator('#command-profile-save').click();

  await expect(select).toHaveValue('FOTW');
  await expect(picked(page)).toHaveText('FOTW');
  await expect(saveChanges).toBeHidden();
  expect(await page.evaluate(() => window.__getProfiles())).toEqual([{ name: 'FOTW', ...FOTW }]);
  expect(await page.evaluate(() => window.__getActive())).toBe('FOTW');

  // An edit keeps the profile picked but marks it, and offers to save back.
  await page.evaluate((l) => window.__editLists({ ...l, init_commands: ['mirv_fov 100'] }), FOTW);
  await expect(select).toHaveValue('FOTW');
  await expect(picked(page)).toHaveText('FOTW (edited)');
  await expect(saveChanges).toBeVisible();

  await saveChanges.click();
  await expect(picked(page)).toHaveText('FOTW');
  await expect(saveChanges).toBeHidden();
  expect((await page.evaluate(() => window.__getProfiles()))[0].init_commands).toEqual(['mirv_fov 100']);
});

test('picking a profile replaces both lists, and Undo puts them back', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(([fotw, clean]) => {
    window.__setProfiles([{ name: 'FOTW', ...fotw }, { name: 'Clean HUD', ...clean }], 'Clean HUD');
    window.__editLists(clean);
  }, [FOTW, CLEAN]);
  await expect(page.locator('#command-profile-select')).toHaveValue('Clean HUD');

  await page.locator('#command-profile-select').selectOption('FOTW');
  expect(await page.evaluate(() => window.__lists())).toEqual(FOTW);
  expect(await page.evaluate(() => window.__getActive())).toBe('FOTW');

  await page.locator('#toast-container button', { hasText: 'Undo' }).click();
  expect(await page.evaluate(() => window.__lists())).toEqual(CLEAN);
  await expect(page.locator('#command-profile-select')).toHaveValue('Clean HUD');
});

test('"—" leaves the profile without touching the lists', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate((fotw) => {
    window.__setProfiles([{ name: 'FOTW', ...fotw }], 'FOTW');
    window.__editLists({ ...fotw, init_commands: ['mirv_fov 100'] });
  }, FOTW);
  await expect(picked(page)).toHaveText('FOTW (edited)');

  await page.locator('#command-profile-select').selectOption('');
  await expect(page.locator('#command-profile-select')).toHaveValue('');
  await expect(page.locator('#command-profile-save-changes')).toBeHidden();
  expect((await page.evaluate(() => window.__lists())).init_commands).toEqual(['mirv_fov 100']);
  expect(await page.evaluate(() => window.__getActive())).toBe('');
});

test('rename renames the picked profile, and refuses a name another profile has', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(([fotw, clean]) => {
    window.__setProfiles([{ name: 'FOTW', ...fotw }, { name: 'Clean HUD', ...clean }], 'FOTW');
    window.__editLists(fotw);
  }, [FOTW, CLEAN]);

  await page.locator('#command-profile-name').fill('clean hud');
  await page.locator('#command-profile-rename').click();
  await expect(page.locator('#toast-container')).toContainText('There is already a profile named "clean hud".');
  expect((await page.evaluate(() => window.__getProfiles())).map((p) => p.name)).toEqual(['Clean HUD', 'FOTW']);

  await page.locator('#command-profile-name').fill('Frag of the Week');
  await page.locator('#command-profile-rename').click();
  await expect(page.locator('#command-profile-select')).toHaveValue('Frag of the Week');
  expect((await page.evaluate(() => window.__getProfiles())).map((p) => p.name)).toEqual(['Clean HUD', 'Frag of the Week']);
  expect(await page.evaluate(() => window.__lists())).toEqual(FOTW);
});

test('delete removes the picked profile and leaves the lists, with Undo', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate((fotw) => {
    window.__setProfiles([{ name: 'FOTW', ...fotw }], 'FOTW');
    window.__editLists(fotw);
  }, FOTW);

  await page.locator('#command-profile-delete').click();
  expect(await page.evaluate(() => window.__getProfiles())).toEqual([]);
  await expect(page.locator('#command-profile-select')).toHaveValue('');
  expect(await page.evaluate(() => window.__lists())).toEqual(FOTW);

  await page.locator('#toast-container button', { hasText: 'Undo' }).click();
  expect((await page.evaluate(() => window.__getProfiles())).map((p) => p.name)).toEqual(['FOTW']);
  await expect(page.locator('#command-profile-select')).toHaveValue('FOTW');
});
