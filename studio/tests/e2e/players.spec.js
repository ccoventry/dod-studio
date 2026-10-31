// players_ui.js — Configuration > Players (#428).
import { test, expect } from '@playwright/test';

const M00CAT = '76561197972576011';

async function loadHarness(page, { pngs = [], sprites = [], spriteReadable = true } = {}) {
  await page.addInitScript(({ pngs, sprites, spriteReadable, m00cat }) => {
    // A 256x256 sprite whose 16 tiles are 16 greys, as the backend sends it.
    const sprite = () => {
      const bytes = new Uint8Array(256 * 256 * 4);
      for (let y = 0; y < 256; y += 1) {
        for (let x = 0; x < 256; x += 1) {
          const tile = Math.floor(y / 64) * 4 + Math.floor(x / 64);
          bytes.set([tile * 16, tile * 16, tile * 16, 255], (y * 256 + x) * 4);
        }
      }
      let text = '';
      for (const b of bytes) text += String.fromCharCode(b);
      return { width: 256, height: 256, rgba: btoa(text) };
    };
    window.__mockInvokeHandlers = {
      list_logo_pngs: () => pngs,
      list_crosshair_sprites: () => window.__sprites ?? sprites,
      crosshair_sprite_image: () => (spriteReadable ? sprite() : Promise.reject('bad sprite')),
      set_player_overrides: () => 1,
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
  }, { pngs, sprites, spriteReadable, m00cat: M00CAT });
  await page.goto('/tests/e2e/players.html');
  await page.waitForFunction(() => window.__harnessReady === true);
  await page.evaluate(() => window.__refresh());
}

const calls = (page, cmd) => page.evaluate((c) => window.__mockInvocations.filter((i) => i.cmd === c), cmd);

test('adds a queue player, gives them a spray and a crosshair tile, and removes them', async ({ page }) => {
  await loadHarness(page, { pngs: ['chris_teamwork.png', 'team_allies.png'], sprites: ['customXHair.spr', 'mine.spr'] });
  const [list] = await calls(page, 'list_crosshair_sprites');
  expect(list.args).toEqual({ gamePath: 'C:/Games/Half-Life/hl.exe', gameFolder: 'dod' });
  await expect(page.locator('#players-players-status')).toHaveText("2 players in the queue's 2 demos");
  // Its bar (#958) is gone once every demo is read.
  await expect(page.locator('#players-players-bar')).toHaveJSProperty('hidden', true);

  const add = page.locator('#players-add-btn');
  await expect(add).toBeDisabled();
  await page.locator('#players-player-input').fill('dicE[: :]m00cat :D / m00cat (STEAM_0:1:6155141)');
  await add.click();
  const row = page.locator('.players-row:not(.players-head)');
  await expect(row).toHaveCount(1);
  await expect(row.locator('.players-name')).toHaveText('dicE[: :]m00cat :D (STEAM_0:1:6155141)');

  await row.locator('.players-spray').selectOption('chris_teamwork.png');
  await row.locator('.players-crosshair select').selectOption('customXHair.spr');
  // Picking a sprite starts on tile 1; the picker shows the 4x4 grid.
  await expect(row.locator('.players-tile')).toContainText('Tile 1');
  await row.locator('.players-tile').click();
  const cells = page.locator('.players-picker .players-cell');
  await expect(cells).toHaveCount(16);
  await expect(page.locator('.players-picker .players-cell.selected')).toHaveText('1');
  await cells.nth(6).click();
  await expect(page.locator('.players-picker')).toHaveCount(0);
  expect(await page.evaluate(() => window.__players().players)).toEqual([
    { steam_id: M00CAT, name: 'dicE[: :]m00cat :D', png: 'chris_teamwork.png', crosshair_sprite: 'customXHair.spr', crosshair_tile: 7, use_as: '', nameplate: '' },
  ]);
  await expect(row.locator('.players-tile')).toContainText('Tile 7');

  // A typed name is a name row.
  await page.locator('#players-player-input').fill('Some Name');
  await add.click();
  expect(await page.evaluate(() => window.__players().players[1].name)).toBe('Some Name');

  await row.first().locator('.players-x').click();
  expect(await page.evaluate(() => window.__players().players.map((p) => p.name))).toEqual(['Some Name']);
  expect(await page.evaluate(() => window.__persisted)).toBe(6);
});

test('the Use in the game box saves and goes to a running game as the cvar', async ({ page }) => {
  await loadHarness(page, { pngs: ['a.png'] });
  const box = page.locator('#players-use-in-game');
  await expect(box).toBeChecked();
  await box.uncheck();
  expect(await page.evaluate(() => window.__players().use_in_game)).toBe(false);
  expect((await calls(page, 'set_player_overrides')).map((c) => c.args)).toEqual([{ on: false }]);
  await expect(page.locator('#players-warning')).toContainText('Use in the game is off');
  await box.check();
  expect((await calls(page, 'set_player_overrides')).map((c) => c.args)).toEqual([{ on: false }, { on: true }]);
  expect(await page.evaluate(() => window.__persisted)).toBe(2);
});

test('fallbacks: HLTV sprays default to one PNG, the crosshair to the game\'s own', async ({ page }) => {
  await loadHarness(page, { pngs: ['team_allies.png'], sprites: ['customXHair.spr'] });
  await expect(page.locator('#players-hltv-fallback')).toHaveValue('png');
  await expect(page.locator('#players-hltv-fallback option')).toHaveText(['One fallback spray', 'No spray']);
  await expect(page.locator('#players-pov-fallback')).toHaveValue('own');
  await expect(page.locator('#players-crosshair-fallback')).toHaveValue('game');
  await expect(page.locator('#players-crosshair-fallback-pick')).toBeHidden();
  await expect(page.locator('#players-warning')).toContainText('HLTV demos: pick the fallback spray');

  await page.locator('#players-hltv-fallback-png select').selectOption('team_allies.png');
  await page.locator('#players-crosshair-fallback').selectOption('sprite');
  await expect(page.locator('#players-crosshair-fallback-pick')).toBeVisible();
  await page.locator('#players-crosshair-fallback-pick .players-tile').click();
  await page.locator('#players-crosshair-fallback-picker .players-cell').nth(15).click();
  const saved = await page.evaluate(() => window.__players());
  expect(saved.hltv_fallback_png).toBe('team_allies.png');
  expect(saved.crosshair_fallback).toBe('sprite');
  expect(saved.crosshair_fallback_sprite).toBe('customXHair.spr');
  expect(saved.crosshair_fallback_tile).toBe(16);
  await expect(page.locator('#players-warning')).toBeHidden();
});

test('a sprite that cannot be read is picked by number', async ({ page }) => {
  await loadHarness(page, { pngs: ['a.png'], sprites: ['broken.spr'], spriteReadable: false });
  await page.evaluate(() => window.__setPlayers({
    players: [{ steam_id: '', name: 'Bot', png: '', crosshair_sprite: 'broken.spr', crosshair_tile: 2 }],
  }));
  await page.locator('#players-list .players-tile').click();
  await expect(page.locator('#players-list .players-grid')).toHaveClass(/players-grid-numbers/);
  await expect(page.locator('.players-picker-note')).toContainText('pick by number');
});

test('no preview is asked for before the hl.exe path is known, and previews load once it is (#852)', async ({ page }) => {
  await loadHarness(page, { pngs: ['a.png'], sprites: ['customXHair.spr'] });
  // As at start-up, when settings render the tab before the path is wired.
  await page.locator('#hl-path-input').fill('');
  await page.evaluate(() => window.__refresh());
  await page.evaluate(() => window.__setPlayers({
    players: [{ steam_id: '', name: 'Bot', png: '', crosshair_sprite: 'customXHair.spr', crosshair_tile: 2 }],
  }));
  await expect(page.locator('#players-list .players-tile')).toContainText('Tile 2');
  const before = (await calls(page, 'crosshair_sprite_image')).length;
  expect((await calls(page, 'crosshair_sprite_image')).filter((c) => !c.args.gamePath)).toEqual([]);

  await page.locator('#hl-path-input').fill('C:/Games/Half-Life/hl.exe');
  await page.evaluate(() => window.__refresh());
  const sent = await calls(page, 'crosshair_sprite_image');
  expect(sent.length).toBeGreaterThan(before);
  for (const c of sent) expect(c.args.gamePath).toBe('C:/Games/Half-Life/hl.exe');
  // Tile 2 is grey 16 in the mock sprite.
  await expect.poll(() => page.evaluate(() => {
    const preview = document.querySelector('#players-list .players-tile-preview');
    return Array.from(preview.getContext('2d').getImageData(16, 16, 1, 1).data);
  })).toEqual([16, 16, 16, 255]);
});

test('says when logos has no PNG, and marks files that are gone', async ({ page }) => {
  await loadHarness(page, { sprites: ['customXHair.spr'] });
  await page.evaluate((id) => window.__setPlayers({
    players: [{ steam_id: id, name: 'm00cat', png: 'gone.png', crosshair_sprite: 'gone.spr', crosshair_tile: 3 }],
    hltv_fallback: 'none',
  }), M00CAT);
  await expect(page.locator('#players-warning')).toContainText("No PNG in the game's logos folder");
  await expect(page.locator('#players-warning')).toContainText('Not in the logos folder: gone.png');
  await expect(page.locator('#players-warning')).toContainText('Not in the sprites folder: gone.spr');
  await expect(page.locator('.players-spray option:checked')).toHaveText('gone.png (not in logos)');
  await expect(page.locator('#players-list .players-crosshair select option:checked')).toHaveText('gone.spr (not in sprites)');
  expect(await page.evaluate(() => window.__persisted)).toBe(0);
});

test('the spray size defaults to 64 and a row can keep the spray from the demo', async ({ page }) => {
  await loadHarness(page, { pngs: ['a.png'] });
  const size = page.locator('#players-spray-size');
  await expect(size).toHaveValue('64');
  await expect(size.locator('option')).toHaveCount(3);
  await size.selectOption('96');
  expect(await page.evaluate(() => window.__players().spray_size)).toBe(96);

  await page.locator('#players-player-input').fill('Some Name');
  await page.locator('#players-add-btn').click();
  const spray = page.locator('#players-list .players-spray');
  await expect(spray.locator('option[value="own"]')).toHaveText('From the demo');
  await expect(spray.locator('option[value="own"]')).toHaveAttribute('title', /fallback for players not listed/);
  await spray.selectOption('own');
  expect(await page.evaluate(() => window.__players().players[0].png)).toBe('own');
  // Not a file: no "not in logos" warning for it.
  await expect(page.locator('#players-warning')).not.toContainText('own');
});

// "Make your own crosshair…" and "Add a crosshair file…" (#842).
test('Make your own crosshair opens the creator in the browser', async ({ page }) => {
  await loadHarness(page, { sprites: ['customXHair.spr'] });
  await page.locator('#players-make-crosshair-btn').click();
  await expect.poll(async () => (await calls(page, 'plugin:opener|open_url')).map((c) => c.args.url))
    .toEqual(['https://vertexs-custom-crosshair-creator.replit.app/']);
});

test('Add a crosshair file: picks the ZIP, names it, copies it and lists it', async ({ page }) => {
  await loadHarness(page, { sprites: ['customXHair.spr', 'customxhair-taken.spr'] });
  await page.evaluate(() => {
    // The picker stays open until the test answers it.
    window.__mockInvokeHandlers['plugin:dialog|open'] = () => new Promise((resolve) => { window.__answerPick = resolve; });
    window.__mockInvokeHandlers.add_crosshair_file = ({ name }) => {
      window.__sprites = ['customXHair.spr', 'customxhair-taken.spr', 'customxhair-red-dot.spr'];
      return name === 'Red Dot' ? 'customxhair-red-dot.spr' : Promise.reject('unexpected name');
    };
  });
  const add = page.locator('#players-add-crosshair-btn');
  await add.click();
  // Disabled while the picker is open.
  await expect(add).toBeDisabled();
  const [pick] = await calls(page, 'plugin:dialog|open');
  expect(pick.args.options.filters[0].extensions).toEqual(['spr', 'zip']);
  await page.evaluate(() => window.__answerPick('C:\\Downloads\\customxhair (2).zip'));
  await expect(add).toBeEnabled();

  const form = page.locator('#players-add-crosshair-form');
  const name = page.locator('#players-add-crosshair-name');
  const save = page.locator('#players-add-crosshair-save');
  await expect(form).toBeVisible();
  await expect(name).toHaveValue('2');
  await expect(page.locator('#players-add-crosshair-file')).toHaveText('Saved as customxhair-2.spr');

  await name.fill('TAKEN');
  await expect(page.locator('#players-add-crosshair-file')).toContainText("There's already a crosshair called customxhair-taken.spr");
  await expect(save).toBeDisabled();
  await name.fill('  ');
  await expect(save).toBeDisabled();
  await name.fill('Red Dot');
  await expect(page.locator('#players-add-crosshair-file')).toHaveText('Saved as customxhair-red-dot.spr');
  await save.click();

  await expect(page.locator('#players-add-crosshair-status')).toHaveText('Added customxhair-red-dot.spr. Pick it in a crosshair list above.');
  await expect(form).toBeHidden();
  const [copy] = await calls(page, 'add_crosshair_file');
  expect(copy.args).toEqual({
    gamePath: 'C:/Games/Half-Life/hl.exe',
    gameFolder: 'dod',
    source: 'C:\\Downloads\\customxhair (2).zip',
    name: 'Red Dot',
  });
  // The crosshair lists offer it now.
  await page.locator('#players-crosshair-fallback').selectOption('sprite');
  await expect(page.locator('#players-crosshair-fallback-pick select option[value="customxhair-red-dot.spr"]')).toHaveCount(1);
});

test('Add a crosshair file: a file that is not a crosshair says why and keeps the name row', async ({ page }) => {
  await loadHarness(page, { sprites: ['customXHair.spr'] });
  await page.evaluate(() => {
    window.__mockInvokeHandlers['plugin:dialog|open'] = () => 'C:\\Downloads\\notes.zip';
    window.__mockInvokeHandlers.add_crosshair_file = () => Promise.reject('C:\\Downloads\\notes.zip: the ZIP holds no crosshair (.spr) file');
  });
  await page.locator('#players-add-crosshair-btn').click();
  await expect(page.locator('#players-add-crosshair-name')).toHaveValue('notes');
  await page.locator('#players-add-crosshair-save').click();
  await expect(page.locator('#players-add-crosshair-status')).toContainText("Couldn't add the crosshair:");
  await expect(page.locator('#players-add-crosshair-status')).toContainText('no crosshair (.spr) file');
  await expect(page.locator('#players-add-crosshair-form')).toBeVisible();
  await expect(page.locator('#players-add-crosshair-save')).toBeEnabled();
  // Cancel closes the row.
  await page.locator('#players-add-crosshair-cancel').click();
  await expect(page.locator('#players-add-crosshair-form')).toBeHidden();
});

test('Add a crosshair file needs the Half-Life executable first', async ({ page }) => {
  await loadHarness(page);
  await page.locator('#hl-path-input').fill('');
  await page.locator('#players-add-crosshair-btn').click();
  await expect(page.locator('#players-add-crosshair-status')).toContainText('Set the Half-Life executable first');
  expect(await calls(page, 'plugin:dialog|open')).toEqual([]);
});
