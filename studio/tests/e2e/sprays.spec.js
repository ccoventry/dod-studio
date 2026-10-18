// sprays_ui.js — Configuration > Sprays (#428).
import { test, expect } from '@playwright/test';

const M00CAT = '76561197972576011';

async function loadHarness(page, pngs) {
  await page.addInitScript(({ pngs, m00cat }) => {
    window.__mockInvokeHandlers = {
      list_logo_pngs: () => pngs,
      // Answers the way the backend does: one event per demo, then resolves.
      index_demo_players: ({ paths, requestId, lane }) => {
        const players = {
          'C:/demos/a.dem': [{ id: m00cat, name: 'dicE[: :]m00cat :D', recorder: true }, { id: 'CONNECTION_2', name: 'Cpl. Preddy' }],
          'C:/demos/b.dem': [{ id: m00cat, name: 'm00cat' }],
        };
        for (const path of paths) {
          window.__mockEmit('demo_players', { lane, requestId, path, players: players[path] || [] });
        }
        return paths.length;
      },
    };
  }, { pngs, m00cat: M00CAT });
  await page.goto('/tests/e2e/sprays.html');
  await page.waitForFunction(() => window.__harnessReady === true);
  await page.evaluate(() => window.__refresh());
}

test('picks a queue player and a PNG, saves the row, and removes it', async ({ page }) => {
  await loadHarness(page, ['chris_teamwork.png', 'team_allies.png']);
  const [call] = await page.evaluate(() => window.__mockInvocations.filter((i) => i.cmd === 'list_logo_pngs'));
  expect(call.args).toEqual({ gamePath: 'C:/Games/Half-Life/hl.exe', gameFolder: 'dod' });
  await expect(page.locator('#sprays-players-status')).toHaveText("2 players in the queue's 2 demos");
  await expect(page.locator('#sprays-player-options option')).toHaveCount(2);

  const add = page.locator('#sprays-add-btn');
  await expect(add).toBeDisabled();
  await page.locator('#sprays-player-input').fill('dicE[: :]m00cat :D / m00cat (STEAM_0:1:6155141)');
  await page.locator('#sprays-add-png').selectOption('chris_teamwork.png');
  await expect(add).toBeEnabled();
  await add.click();

  expect(await page.evaluate(() => window.__sprays().players)).toEqual([
    { steam_id: M00CAT, name: 'dicE[: :]m00cat :D', png: 'chris_teamwork.png' },
  ]);
  expect(await page.evaluate(() => window.__persisted)).toBe(1);
  const row = page.locator('.sprays-row');
  await expect(row).toHaveCount(1);
  await expect(row.locator('.sprays-player')).toHaveText('dicE[: :]m00cat :D (STEAM_0:1:6155141)');

  // A typed name is a name row.
  await page.locator('#sprays-player-input').fill('Some Name');
  await page.locator('#sprays-add-png').selectOption('team_allies.png');
  await add.click();
  expect(await page.evaluate(() => window.__sprays().players[1])).toEqual({ steam_id: '', name: 'Some Name', png: 'team_allies.png' });

  await row.first().locator('.sprays-x').click();
  expect(await page.evaluate(() => window.__sprays().players.map((p) => p.name))).toEqual(['Some Name']);
  expect(await page.evaluate(() => window.__persisted)).toBe(3);
});

test('fallbacks: HLTV defaults to one PNG and never offers their own', async ({ page }) => {
  await loadHarness(page, ['team_allies.png']);
  await expect(page.locator('#sprays-hltv-fallback')).toHaveValue('png');
  await expect(page.locator('#sprays-hltv-fallback option')).toHaveText(['One fallback spray', 'No spray']);
  await expect(page.locator('#sprays-pov-fallback')).toHaveValue('own');
  await expect(page.locator('#sprays-pov-fallback-png')).toBeHidden();
  await expect(page.locator('#sprays-warning')).toContainText('HLTV demos: pick the fallback spray');

  await page.locator('#sprays-hltv-fallback-png select').selectOption('team_allies.png');
  await page.locator('#sprays-pov-fallback').selectOption('none');
  const saved = await page.evaluate(() => window.__sprays());
  expect(saved.hltv_fallback_png).toBe('team_allies.png');
  expect(saved.pov_fallback).toBe('none');
  await expect(page.locator('#sprays-warning')).toBeHidden();
});

test('says when logos has no PNG, and marks a saved PNG that is gone', async ({ page }) => {
  await loadHarness(page, []);
  await page.evaluate((id) => window.__setSprays({
    players: [{ steam_id: id, name: 'm00cat', png: 'gone.png' }],
    hltv_fallback: 'none',
  }), M00CAT);
  await expect(page.locator('#sprays-warning')).toContainText("No PNG in the game's logos folder");
  await expect(page.locator('#sprays-warning')).toContainText('Not in the logos folder: gone.png');
  await expect(page.locator('.sprays-row select option')).toHaveText(['gone.png (not in logos)']);
  expect(await page.evaluate(() => window.__persisted)).toBe(0);
});
