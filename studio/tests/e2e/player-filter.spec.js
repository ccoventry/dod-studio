// master_pane.js — the Master Queue's player filter (#174).
//
// Pins the two ways a demo gets its recorder: stored by the scan
// (`recorder_id`), or looked up through the player index for a demo from a
// project saved before scans stored one. An HLTV demo (null recorder) must
// drop out as soon as a player is picked, and must never be looked up.
import { test, expect } from '@playwright/test';

const ME = '76561197977930126';

const DEMOS = [
  { name: 'mine.dem', path: 'C:/d/mine.dem', local_player_index: 0, recorder_id: ME, recorder_name: 'chris', streaks: [] },
  { name: 'theirs.dem', path: 'C:/d/theirs.dem', local_player_index: 1, recorder_id: 'PLAYER_93', recorder_name: 'Las1k', streaks: [] },
  { name: 'hltv.dem', path: 'C:/d/hltv.dem', local_player_index: null, recorder_id: null, recorder_name: null, streaks: [] },
  // Saved before #174: no recorder fields at all.
  { name: 'old.dem', path: 'C:/d/old.dem', local_player_index: 0, streaks: [] },
];

async function loadHarness(page) {
  await page.goto('/tests/e2e/player-filter.html');
  await page.waitForFunction(() => window.__harnessReady === true);
  await page.evaluate((demos) => window.__render(demos), DEMOS);
}

test('the select lists each recorder once and filters to their demos', async ({ page }) => {
  await loadHarness(page);
  const select = page.locator('#master-player-filter');
  await expect(select.locator('option')).toHaveText(['All players', 'chris (STEAM_0:0:8832199)', 'Las1k']);

  await select.selectOption(ME);
  expect(await page.evaluate(() => window.__visibleNames())).toEqual(['mine.dem']);

  await select.selectOption('');
  expect(await page.evaluate(() => window.__visibleNames())).toHaveLength(4);
});

test('an older demo is looked up once, and joins its recorder when the answer arrives', async ({ page }) => {
  await loadHarness(page);
  const lookups = await page.evaluate(() =>
    window.__mockInvocations.filter((c) => c.cmd === 'index_demo_players').map((c) => c.args));
  expect(lookups).toHaveLength(1);
  expect(lookups[0].paths).toEqual(['C:/d/old.dem']);
  expect(lookups[0].lane).toBe('queue');

  await page.locator('#master-player-filter').selectOption(ME);
  expect(await page.evaluate(() => window.__visibleNames())).toEqual(['mine.dem']);

  await page.evaluate((me) => window.__mockEmit('demo_players', {
    lane: 'queue', requestId: 1, path: 'C:/d/old.dem', demoType: 'POV',
    players: [{ id: me, name: '[TAG] chris', recorder: true }, { id: 'PLAYER_93', name: 'Las1k', recorder: false }],
  }), ME);
  expect(await page.evaluate(() => window.__visibleNames())).toEqual(['mine.dem', 'old.dem']);
  // Equal counts: names sort alphabetically.
  await expect(page.locator('#master-player-filter option')).toHaveText(
    ['All players', '[TAG] chris / chris (STEAM_0:0:8832199)', 'Las1k']);

  // Re-rendering doesn't ask again.
  await page.evaluate((demos) => window.__render(demos), DEMOS);
  const again = await page.evaluate(() =>
    window.__mockInvocations.filter((c) => c.cmd === 'index_demo_players').length);
  expect(again).toBe(1);
});

test('an analyzer-lane answer is ignored by the queue', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate((me) => window.__mockEmit('demo_players', {
    lane: 'analyzer', requestId: 1, path: 'C:/d/old.dem', demoType: 'POV',
    players: [{ id: me, name: 'chris', recorder: true }],
  }), ME);
  await page.locator('#master-player-filter').selectOption(ME);
  expect(await page.evaluate(() => window.__visibleNames())).toEqual(['mine.dem']);
});
