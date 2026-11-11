// players_ui.js's Nameplate column (#429): a PNG picked with the file dialog
// is saved on the player's row, shown by its file name, still there after a
// reload, marked plainly when the file is gone, and cleared with ×.
import { test, expect } from '@playwright/test';

const M00CAT = '76561197972576011';
const PNG = 'D:\\movie\\nameplates\\m00cat.png';

async function loadHarness(page, { missing = [] } = {}) {
  await page.addInitScript(({ missing }) => {
    window.__mockInvokeHandlers = {
      list_logo_pngs: () => [],
      list_crosshair_sprites: () => [],
      index_demo_players: () => 0,
      set_player_overrides: () => 1,
      pick_path_states: ({ paths }) => paths.map(() => null),
      pick_user_folders: () => ({}),
      files_exist: ({ paths }) => paths.map((p) => !missing.includes(p)),
    };
  }, { missing });
  // The Use as harness: the Players tab, saved to localStorage.
  await page.goto('/tests/e2e/player-use-as.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const calls = (page, cmd) => page.evaluate((c) => window.__mockInvocations.filter((i) => i.cmd === c), cmd);

async function addM00cat(page) {
  await page.locator('#players-player-input').fill(M00CAT);
  await page.locator('#players-add-btn').click();
}

test('a picked nameplate is saved by SteamID, shown by name, and survives a reload', async ({ page }) => {
  await loadHarness(page);
  await addM00cat(page);
  await expect(page.locator('.players-head span').nth(2)).toHaveText('Nameplate');

  await page.evaluate((png) => { window.__mockInvokeHandlers['plugin:dialog|open'] = () => png; }, PNG);
  const pick = page.locator('.players-row:not(.players-head) .players-nameplate-pick');
  await expect(pick).toHaveText('Pick a PNG…');
  await pick.click();
  await expect(pick).toHaveText('m00cat.png');
  await expect(pick).toHaveAttribute('title', PNG);
  const [dialog] = await calls(page, 'plugin:dialog|open');
  expect(dialog.args.options.filters[0].extensions).toEqual(['png']);
  const saved = (await page.evaluate(() => window.__players())).players[0];
  expect(saved).toMatchObject({ steam_id: M00CAT, nameplate: PNG });

  await page.reload();
  await page.waitForFunction(() => window.__harnessReady === true);
  await expect(page.locator('.players-nameplate-pick')).toHaveText('m00cat.png');

  await page.locator('.players-nameplate-clear').click();
  await expect(page.locator('.players-nameplate-pick')).toHaveText('Pick a PNG…');
  expect((await page.evaluate(() => window.__players())).players[0].nameplate).toBe('');
});

test('a nameplate whose file is gone says so', async ({ page }) => {
  await loadHarness(page, { missing: [PNG] });
  await addM00cat(page);
  await page.evaluate((png) => { window.__mockInvokeHandlers['plugin:dialog|open'] = () => png; }, PNG);
  const pick = page.locator('.players-nameplate-pick');
  await pick.click();
  await expect(pick).toHaveText('m00cat.png (missing)');
  await expect(pick).toHaveClass(/missing/);
  await expect(pick).toHaveAttribute('title', /isn't there any more/);
});

test('a player added by name has no SteamID, so no nameplate', async ({ page }) => {
  await loadHarness(page);
  await page.locator('#players-player-input').fill('Some Bot');
  await page.locator('#players-add-btn').click();
  await expect(page.locator('.players-nameplate-pick')).toBeDisabled();
});
