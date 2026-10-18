// command_players_ui.js — a Scheduled Command's Players cell.
import { test, expect } from '@playwright/test';

const A = '76561197961831029';

async function loadHarness(page) {
  await page.goto('/tests/e2e/command-players.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

test('empty reads Everyone; picking, typing and removing players edit the row', async ({ page }) => {
  await loadHarness(page);
  const cell = page.locator('.command-players');
  const box = page.locator('.command-players-input');
  await expect(cell).toHaveClass(/command-players-everyone/);
  await expect(box).toHaveAttribute('placeholder', 'Everyone');

  // The picker lists the queue's POV recorder and HLTV player.
  await box.focus();
  const options = await page.locator('#command-players-options option').evaluateAll((os) => os.map((o) => o.value));
  expect(options).toEqual(['Brain (STEAM_0:1:6154641)', 'm00cat (STEAM_0:1:782650)']);

  // Picking an option adds it at once.
  await box.fill('m00cat (STEAM_0:1:782650)');
  await expect(page.locator('.command-player-chip')).toHaveCount(1);
  await expect(cell).not.toHaveClass(/command-players-everyone/);

  // A typed name goes in on Enter.
  await box.fill('Someone');
  await box.press('Enter');
  await expect(page.locator('.command-player-chip')).toHaveText(['m00cat×', 'Someone×']);
  expect(await page.evaluate(() => window.__items()[0].players)).toEqual([
    { id: A, name: 'm00cat' },
    { id: null, name: 'Someone' },
  ]);

  await page.locator('.command-player-remove').first().click();
  expect(await page.evaluate(() => window.__items()[0].players)).toEqual([{ id: null, name: 'Someone' }]);
  expect(await page.evaluate(() => window.__changes)).toBe(3);
});
