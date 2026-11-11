// players_ui.js's Use as column (#686): a name set there replaces {name} in
// the clip name preview, clearing it puts the in-game name back, and it is
// still there after a reload.
import { test, expect } from '@playwright/test';

const M00CAT = '76561197972576011';

async function loadHarness(page) {
  await page.addInitScript(() => {
    window.__mockInvokeHandlers = {
      list_logo_pngs: () => [],
      list_crosshair_sprites: () => [],
      index_demo_players: () => 0,
      set_player_overrides: () => 1,
    };
  });
  await page.goto('/tests/e2e/player-use-as.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

test('Use as changes the clip name preview, clears back, and survives a reload', async ({ page }) => {
  await loadHarness(page);
  const preview = page.locator('#config-clip-name-preview');
  await expect(preview).toContainText('Preview: dicE[_ _]m00cat _D_2k');

  await page.locator('#players-player-input').fill(M00CAT);
  await page.locator('#players-add-btn').click();
  const row = page.locator('.players-row:not(.players-head)');
  await expect(page.locator('.players-head span').nth(1)).toHaveText('Use as');
  await expect(page.locator('.players-head span').nth(1)).toHaveAttribute('title', /instead of the in-game name/);

  const useAs = row.locator('.players-use-as');
  await useAs.fill('  Chris  ');
  await useAs.press('Enter');
  await expect(preview).toContainText('Preview: Chris_2k');
  expect((await page.evaluate(() => window.__players())).players[0].use_as).toBe('Chris');

  await page.reload();
  await page.waitForFunction(() => window.__harnessReady === true);
  await expect(page.locator('.players-use-as')).toHaveValue('Chris');
  await expect(preview).toContainText('Preview: Chris_2k');

  await page.locator('.players-use-as').fill('');
  await page.locator('.players-use-as').press('Enter');
  await expect(preview).not.toContainText('Chris');
  expect((await page.evaluate(() => window.__players())).players[0].use_as).toBe('');
});

test('a player added by name has no SteamID, so Use as is off for them', async ({ page }) => {
  await loadHarness(page);
  await page.locator('#players-player-input').fill('Some Bot');
  await page.locator('#players-add-btn').click();
  await expect(page.locator('.players-row:not(.players-head) .players-use-as')).toBeDisabled();
});
