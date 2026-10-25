// The Review box's HLTV choices (#822) and "Players I pick" (#828) —
// driven against tests/e2e/review-hltv.html. See tests/e2e/README.md.
import { test, expect } from '@playwright/test';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/review-hltv.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const hltv = (page) => page.locator('#review-hltv-select');
const box = (page) => page.locator('#review-hltv-pick');
const chips = (page) => page.locator('#review-hltv-chips .review-hltv-chip');

async function pickPlayers(page) {
  await hltv(page).selectOption('picked');
}

test.describe('Review: HLTV players', () => {
  test('the box keeps its choices, with Players I pick after Everyone; the pick shows only for it', async ({ page }) => {
    await gotoHarness(page);
    await expect(hltv(page).locator('option')).toHaveText([
      'Players missing their POV for this half',
      'Players with a POV demo in the queue',
      'Everyone',
      'Players I pick',
      'm00cat (STEAM_0:0:1)',
    ]);
    const chipRow = page.locator('#review-hltv-chips');
    await expect(box(page)).toHaveJSProperty('hidden', true);
    await expect(chipRow).toHaveJSProperty('hidden', true);
    await pickPlayers(page);
    await expect(box(page)).toHaveJSProperty('hidden', false);
    await expect(chipRow).toHaveJSProperty('hidden', false);
    await expect(chipRow).toHaveText('Nobody picked yet');
    await expect.poll(() => page.evaluate(() => window.__changes)).toBe(1);
    await hltv(page).selectOption('all');
    await expect(box(page)).toHaveJSProperty('hidden', true);
    await expect(chipRow).toHaveJSProperty('hidden', true);
  });

  test('the chips have a line of their own, so picking players never moves the lists (#845)', async ({ page }) => {
    await gotoHarness(page);
    await pickPlayers(page);
    const team = page.locator('#review-hltv-add-team');
    const before = await team.boundingBox();
    const add = page.locator('#review-hltv-add-player');
    for (let i = 0; i < 3; i++) {
      const value = await add.locator('option').nth(1).getAttribute('value');
      await add.selectOption(value);
    }
    await expect(chips(page)).toHaveCount(3);
    expect(await team.boundingBox()).toEqual(before);
    // The lists stay side by side.
    const player = await add.boundingBox();
    expect(Math.abs(player.y - before.y)).toBeLessThan(2);
  });

  test('lists picked on an empty queue fill once demos arrive', async ({ page }) => {
    await gotoHarness(page);
    await page.evaluate(() => { window.__emptyQueue(true); window.__pick.refresh(); });
    await pickPlayers(page);
    const add = page.locator('#review-hltv-add-player');
    await expect(add).toBeDisabled();
    await expect(add.locator('option')).toHaveText(['No HLTV highlights in the queue']);
    await expect(page.locator('#review-hltv-add-team option')).toHaveText(['No teams']);
    // Both lists' fixed widths show the whole line beside the ~12 px arrow
    // (#845: the team list cut "No teams in the HLTV demos" short).
    const cut = await page.evaluate(() => {
      const ctx = document.createElement('canvas').getContext('2d');
      return [...document.querySelectorAll('#review-hltv-add-player, #review-hltv-add-team')]
        .filter((list) => {
          const css = getComputedStyle(list);
          ctx.font = `${css.fontSize} ${css.fontFamily}`;
          const room = list.clientWidth - parseFloat(css.paddingLeft) - parseFloat(css.paddingRight) - 12;
          return ctx.measureText(list.options[0].text).width > room;
        })
        .map((list) => list.options[0].text);
    });
    expect(cut).toEqual([]);
    // What main.js's updateDemoFooter does after a scan.
    await page.evaluate(() => { window.__emptyQueue(false); window.__pick.refresh(); });
    await expect(add).toBeEnabled();
    await expect(add.locator('option')).toHaveCount(5);
    await expect(page.locator('#review-hltv-add-team')).toBeEnabled();
  });

  test('lists everyone with an HLTV highlight, one line per SteamID with every name; picked ones show as chips with ×', async ({ page }) => {
    await gotoHarness(page);
    await pickPlayers(page);
    const add = page.locator('#review-hltv-add-player');
    await expect(add.locator('option')).toHaveText([
      'Add a player…',
      'dicE | gorilla / gorilla (STEAM_0:1:1)',
      'dicE | stealth (STEAM_0:1:0)',
      'oldie',
      'overlord (STEAM_0:1:2)',
    ]);
    await add.selectOption('76561197960265731');
    await expect(chips(page)).toHaveText(['dicE | gorilla / gorilla (STEAM_0:1:1)×']);
    // A picked player leaves the list.
    await expect(add.locator('option')).toHaveCount(4);
    await chips(page).first().locator('.review-hltv-chip-x').click();
    await expect(chips(page)).toHaveCount(0);
    await expect(add.locator('option')).toHaveCount(5);
  });

  test('a team from the Teams list adds its players: by side in the HLTV demo, or by tag', async ({ page }) => {
    await gotoHarness(page);
    await pickPlayers(page);
    const team = page.locator('#review-hltv-add-team');
    await expect(team.locator('option')).toHaveText(['Add a team…', 'DICE (2 players)', 'over (1 player)']);
    await team.selectOption('dicE');
    await expect(chips(page)).toHaveText([
      'dicE | gorilla / gorilla (STEAM_0:1:1)×',
      'dicE | stealth (STEAM_0:1:0)×',
    ]);
  });

  test('a review sends only the picked players\' HLTV highlights, and every POV one', async ({ page }) => {
    await gotoHarness(page);
    await pickPlayers(page);
    await page.locator('#review-hltv-add-player').selectOption('76561197960265731');
    await page.locator('#review-hltv-add-player').selectOption('name:oldie');
    await page.locator('#review-highlights-btn').click();
    await expect.poll(() => page.evaluate(() => window.__sent())).toEqual([
      ['h1.dem:gorilla', 'h1.dem:oldie', 'h2.dem:dicE | gorilla', 'p.dem:'],
    ]);
  });

  test('nobody picked: nothing is sent from the HLTV demos, and the toast says why', async ({ page }) => {
    await gotoHarness(page);
    await page.evaluate(() => { window.__checked = ['h1.dem', 'h2.dem']; });
    await pickPlayers(page);
    await page.locator('#review-highlights-btn').click();
    await expect(page.locator('.toast')).toContainText('nobody is picked');
    expect(await page.evaluate(() => window.__sent())).toEqual([]);
  });

  test('the project file\'s pick comes back, a player not in the queue dimmed; an older file goes back to the default', async ({ page }) => {
    await gotoHarness(page);
    await page.evaluate(() => window.__pick.setState({
      hltv: 'picked',
      hltv_players: [
        { id: '76561197960265729', names: ['dicE | stealth'] },
        { id: '76561197960265799', names: ['gone'] },
      ],
    }));
    await expect(hltv(page)).toHaveValue('picked');
    await expect(chips(page)).toHaveText(['dicE | stealth (STEAM_0:1:0)×', 'gone (STEAM_0:1:35)×']);
    await expect(chips(page).nth(1)).toHaveClass(/review-hltv-chip-away/);
    expect(await page.evaluate(() => window.__pick.getState())).toEqual({
      hltv: 'picked',
      hltv_players: [
        { id: '76561197960265729', names: ['dicE | stealth'] },
        { id: '76561197960265799', names: ['gone'] },
      ],
    });
    await page.evaluate(() => window.__pick.setState(undefined));
    await expect(hltv(page)).toHaveValue('missing');
    await expect(box(page)).toHaveJSProperty('hidden', true);
  });
});
