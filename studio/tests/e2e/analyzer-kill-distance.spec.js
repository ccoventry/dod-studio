// analyzer_pane.js — engagement distance in the Player Details tab (#448): each
// kill in the kill list, the player's average and the per-weapon average. An
// unknown position is a blank cell with a tooltip, never a zero.
import { test, expect } from '@playwright/test';

const DEMO = 'C:/demos/match.dem';
const time = (secs) => ({ real_offset: { secs, nanos: 0 }, viewdemo_offset: { secs, nanos: 0 }, frame_index: 1 });

const kill = (secs, weapon, victim, distance) => ({
  time: time(secs), weapon, killer: 'STEAM_1', victim, killer_team: 'Allies', victim_team: 'Axis',
  teamkill: false, killer_origin: distance === null ? null : [0, 0, 0], victim_origin: [1, 1, 1], distance,
});

const player = (id, name, team, extra = {}) => ({
  id, name, team, connection: { Connected: { client_id: 0 } }, stats: [0, 0, 0], mortality: [],
  kill_streaks: [], weapon_breakdown: {}, ...extra,
});

function report(demoType) {
  return {
    file_name: 'match.dem', file_path: DEMO, file_dir: 'C:/demos', file_size_mb: 80, file_created_unix_secs: 0,
    demo_info: {
      demo_protocol: 5, map_name: 'dod_anzio', network_protocol: 48, playback_time: 600,
      playback_frames: 1000, game_directory: 'dod', demo_type: demoType, map_checksum: 1,
    },
    state: {
      players: [
        player('STEAM_1', 'killer', 'Allies', {
          stats: [0, 3, 0],
          // Three kills in one life: 1000 units (25.4 m), unknown, 600 units (15.2 m).
          kill_streaks: [{ slot: 0, kills: [[time(75), 'K98', 'STEAM_2'], [time(80), 'K98', 'STEAM_3'], [time(85), 'Mp40', 'STEAM_2']] }],
          weapon_breakdown: { K98: [2, 0], Mp40: [1, 0], Spade: [0, 0] },
        }),
        player('STEAM_2', 'victim', 'Axis'),
        player('STEAM_3', 'other victim', 'Axis'),
      ],
      rounds: [], chat_messages: [], team_scores: { timeline: [] }, current_time: time(600),
      allies_are_british: false, clan_match_detected: false,
      kill_positions: [
        kill(75, 'K98', 'STEAM_2', 1000),
        kill(80, 'K98', 'STEAM_3', null),
        kill(85, 'Mp40', 'STEAM_2', 600),
      ],
    },
  };
}

async function openPlayer(page, demoType = 'POV') {
  await page.evaluate((payload) => {
    window.__mockInvokeHandlers.analyze_demo_full = () => payload;
    window.__mockInvokeHandlers.browse_directory = () => ({ path: 'C:/demos', dirs: [], demos: [] });
  }, report(demoType));
  await page.evaluate((path) => window.__openDemo(path), DEMO);
  await page.locator('.analyzer-subtab-btn[data-subtab="player-details"]').click();
  await page.locator('#player-details-select').selectOption('STEAM_1');
}

test.beforeEach(async ({ page }) => {
  await page.goto('/tests/e2e/analyzer-kill-map.html');
  await page.waitForFunction(() => window.__harnessReady === true);
});

test('each kill in the list shows its distance, blank with a tooltip when unknown', async ({ page }) => {
  await openPlayer(page);
  const subrows = page.locator('.killstreak-subrow');
  await expect(subrows).toHaveCount(3);
  await expect(subrows.nth(0)).toContainText('25.4 m');
  await expect(subrows.nth(2)).toContainText('15.2 m');
  const unknown = subrows.nth(1).locator('td').last();
  await expect(unknown).toHaveText('');
  await expect(unknown).toHaveAttribute('title', 'Position unknown');
});

test('the player card and the weapon table average the known kills and say how many', async ({ page }) => {
  await openPlayer(page);
  const card = page.locator('.analyzer-stat-card', { hasText: 'Avg. Distance' });
  await expect(card).toContainText('20.3 m');
  await expect(card).toContainText('2 of 3 kills');

  const rows = page.locator('.analyzer-table').first().locator('tbody tr');
  await expect(rows.filter({ hasText: 'K98' })).toContainText('25.4 m (1 of 2 kills)');
  await expect(rows.filter({ hasText: 'Mp40' })).toContainText('15.2 m (1 of 1 kills)');
  // A weapon with no kills has no distance, and says why on hover.
  const none = rows.filter({ hasText: 'Spade' }).locator('td').last();
  await expect(none).toHaveText('');
  await expect(none).toHaveAttribute('title', 'Position unknown');
});

test('a player nobody measured shows a dash, not 0 m', async ({ page }) => {
  await openPlayer(page);
  await page.locator('#player-details-select').selectOption('STEAM_2');
  const card = page.locator('.analyzer-stat-card', { hasText: 'Avg. Distance' });
  await expect(card).not.toContainText('0.0 m');
  await expect(card).toContainText('0 of 0 kills');
});

test('an HLTV demo shows the same columns', async ({ page }) => {
  await openPlayer(page, 'HLTV');
  await expect(page.locator('.killstreak-subrow').nth(0)).toContainText('25.4 m');
});
