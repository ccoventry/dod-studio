// game_folder_ui.js — Configuration > Paths' Game Folder row (#402).
import { test, expect } from '@playwright/test';

async function loadHarness(page, folders) {
  await page.addInitScript((f) => {
    window.__mockInvokeHandlers = {
      list_game_folders: () => (f === null ? Promise.reject('unreadable') : f),
    };
  }, folders);
  await page.goto('/tests/e2e/game-folder.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

test('lists the folders beside hl.exe and saves the one picked', async ({ page }) => {
  await loadHarness(page, ['dod', 'dod_1_2', 'dod_movies']);
  await page.evaluate(async () => { window.__setGameFolder('dod'); await window.__refresh(); });
  const select = page.locator('#config-game-folder');
  await expect(select).toBeVisible();
  await expect(select).toBeEnabled();
  await expect(select.locator('option')).toHaveText(['dod', 'dod_1_2', 'dod_movies']);
  await expect(select).toHaveValue('dod');
  const [call] = await page.evaluate(() => window.__mockInvocations.filter((i) => i.cmd === 'list_game_folders'));
  expect(call.args).toEqual({ gamePath: 'C:/Games/Half-Life/hl.exe' });

  await select.selectOption('dod_movies');
  expect(await page.evaluate(() => window.__gameFolder())).toBe('dod_movies');
  expect(await page.evaluate(() => window.__persisted)).toBe(1);
});

test('a saved folder that is not there stays, marked, with a warning', async ({ page }) => {
  await loadHarness(page, ['dod']);
  await page.evaluate(async () => { window.__setGameFolder('dod_movies'); await window.__refresh(); });
  const select = page.locator('#config-game-folder');
  await expect(select).toHaveValue('dod_movies');
  await expect(select.locator('option').first()).toHaveText('dod_movies (not found beside hl.exe)');
  await expect(page.locator('#config-game-folder-warning')).toContainText('no "dod_movies" folder');
  expect(await page.evaluate(() => window.__persisted)).toBe(0);
});

test('with nothing to list, the dropdown is disabled on the saved value and says why', async ({ page }) => {
  await loadHarness(page, null);
  await page.evaluate(async () => { window.__setGameFolder('dod_movies'); await window.__refresh(); });
  const select = page.locator('#config-game-folder');
  await expect(select).toBeVisible();
  await expect(select).toBeDisabled();
  await expect(select.locator('option')).toHaveText(['dod_movies']);
  await expect(select).toHaveValue('dod_movies');
  await expect(page.locator('#config-game-folder-warning')).toContainText('Set the Half-Life Executable');
  expect(await page.evaluate(() => window.__gameFolder())).toBe('dod_movies');
  expect(await page.evaluate(() => window.__persisted)).toBe(0);

  // The folders turn up once hl.exe points somewhere real.
  await page.evaluate(() => {
    window.__mockInvokeHandlers.list_game_folders = () => ['dod', 'dod_movies'];
  });
  await page.evaluate(() => window.__refresh());
  await expect(select).toBeEnabled();
  await expect(select.locator('option')).toHaveText(['dod', 'dod_movies']);
  await expect(page.locator('#config-game-folder-warning')).toBeHidden();
});
