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
  await expect(page.locator('#config-game-folder-text')).toBeHidden();
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

test('with nothing to list, a text field takes the name and refuses a path', async ({ page }) => {
  await loadHarness(page, null);
  await page.evaluate(async () => { window.__setGameFolder(''); await window.__refresh(); });
  const text = page.locator('#config-game-folder-text');
  await expect(text).toBeVisible();
  await expect(page.locator('#config-game-folder')).toBeHidden();
  await expect(text).toHaveValue('dod');

  await text.fill('../elsewhere');
  await text.dispatchEvent('change');
  await expect(page.locator('#config-game-folder-warning')).toBeVisible();
  expect(await page.evaluate(() => window.__gameFolder())).toBe('dod');
  expect(await page.evaluate(() => window.__persisted)).toBe(0);

  await text.fill('dod_movies');
  await text.dispatchEvent('change');
  await expect(page.locator('#config-game-folder-warning')).toBeHidden();
  expect(await page.evaluate(() => window.__gameFolder())).toBe('dod_movies');
  expect(await page.evaluate(() => window.__persisted)).toBe(1);
});
