// analyzer_pane.js — the Demo Analyzer's Kill Map tab (#448).
//
// Pins where a death marker lands on the overview (the placement arithmetic
// itself is unit-tested in src/kill_map.test.js; this checks the page uses it
// in the image's own pixels), what the hover says, the engagement-distance
// table, and that a map with no overview still gets the table and a plain
// reason instead of a broken image.
import { test, expect } from '@playwright/test';

const DEMO = 'C:/demos/match.dem';

// A 1024x768 stand-in for dod_anzio.bmp.
const IMAGE = 'data:image/svg+xml,' + encodeURIComponent(
  '<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="768"><rect width="1024" height="768" fill="#887"/></svg>'
);
const ANZIO = {
  placement: { zoom: 1.11, origin: [307.06, 372.72, -334], rotated: false, image: 'overviews/dod_anzio.bmp' },
  image_data_url: IMAGE,
};

const time = (secs) => ({ real_offset: { secs, nanos: 0 }, viewdemo_offset: { secs, nanos: 0 }, frame_index: 1 });

/** native's AnalyzerReportPayload, cut to what the Summary and Kill Map tabs read. */
function report(demoType, kills) {
  return {
    file_name: 'match.dem',
    file_path: DEMO,
    file_dir: 'C:/demos',
    file_size_mb: 80,
    file_created_unix_secs: 0,
    demo_info: {
      demo_protocol: 5, map_name: 'dod_anzio', network_protocol: 48, playback_time: 600,
      playback_frames: 1000, game_directory: 'dod', demo_type: demoType, map_checksum: 1,
    },
    state: {
      players: [
        { id: 'STEAM_1', name: 'killer', team: 'Allies', connection: { Connected: { client_id: 0 } }, stats: [0, 0, 0], mortality: [], kill_streaks: [], weapon_breakdown: {} },
        { id: 'STEAM_2', name: 'victim', team: 'Axis', connection: { Connected: { client_id: 1 } }, stats: [0, 0, 0], mortality: [], kill_streaks: [], weapon_breakdown: {} },
      ],
      rounds: [],
      chat_messages: [],
      team_scores: { timeline: [] },
      current_time: time(600),
      allies_are_british: false,
      clan_match_detected: false,
      kill_positions: kills,
    },
  };
}

const KILLS = [
  {
    time: time(75), weapon: 'K98', killer: 'STEAM_1', victim: 'STEAM_2', killer_team: 'Allies', victim_team: 'Axis',
    teamkill: false, killer_origin: [1307.06, 372.72, -334], victim_origin: [307.06, 372.72, -334], distance: 1000,
  },
  {
    time: time(90), weapon: 'Mk2Grenade', killer: null, victim: 'STEAM_1', killer_team: null, victim_team: 'Allies',
    teamkill: false, killer_origin: null, victim_origin: [407.06, 372.72, -334], distance: null,
  },
  {
    // Out of the recorder's view: no position, so no marker.
    time: time(120), weapon: 'Mp40', killer: 'STEAM_2', victim: 'STEAM_1', killer_team: 'Axis', victim_team: 'Allies',
    teamkill: false, killer_origin: [0, 0, 0], victim_origin: null, distance: null,
  },
];

async function openKillMap(page, { demoType = 'HLTV', kills = KILLS, overview = ANZIO } = {}) {
  await page.evaluate(({ payload, overview }) => {
    window.__mockInvokeHandlers.analyze_demo_full = () => payload;
    window.__mockInvokeHandlers.load_map_overview = () => overview;
    window.__mockInvokeHandlers.browse_directory = () => ({ path: 'C:/demos', dirs: [], demos: [] });
  }, { payload: report(demoType, kills), overview });
  await page.evaluate((path) => window.__openDemo(path), DEMO);
  await page.locator('.analyzer-subtab-btn[data-subtab="kill-map"]').click();
}

test.beforeEach(async ({ page }) => {
  await page.goto('/tests/e2e/analyzer-kill-map.html');
  await page.waitForFunction(() => window.__harnessReady === true);
});

test('places each known death on the overview, in the image\'s own pixels', async ({ page }) => {
  await openKillMap(page);
  const circles = page.locator('.analyzer-killmap circle');
  await expect(circles).toHaveCount(2);
  // The first victim stood exactly on ORIGIN: the centre of a 1024x768 image.
  await expect(circles.nth(0)).toHaveAttribute('cx', '512.0');
  await expect(circles.nth(0)).toHaveAttribute('cy', '384.0');
  await expect(page.locator('.analyzer-timeline-legend')).toContainText('2 of 3 deaths shown.');

  const lookup = await page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'load_map_overview'));
  expect(lookup.args).toEqual({ gamePath: 'C:/games/Half-Life/hl.exe', demoPath: DEMO, mapName: 'dod_anzio' });
});

test('hovering a marker names both players, the weapon and the distance', async ({ page }) => {
  await openKillMap(page);
  await page.locator('.analyzer-killmap circle').nth(0).hover();
  const tooltip = page.locator('.analyzer-killmap .analyzer-timeline-tooltip');
  await expect(tooltip).toBeVisible();
  await expect(tooltip).toContainText('victim killed by killer (K98)');
  await expect(tooltip).toContainText('Distance: 25.4 m');

  await page.locator('.analyzer-killmap circle').nth(1).hover();
  await expect(tooltip).toContainText('killer died');
  await expect(tooltip).not.toContainText('Distance');
});

test('the distance table leaves out kills without both positions', async ({ page }) => {
  await openKillMap(page);
  const rows = page.locator('.analyzer-table tbody tr');
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0)).toContainText('K98');
  await expect(rows.nth(0)).toContainText('25.4 m');
  await expect(rows.nth(1)).toContainText('All weapons');
});

test('a map without an overview says so and still shows the table', async ({ page }) => {
  await openKillMap(page, { overview: null });
  await expect(page.locator('#analyzer-killmap-area')).toContainText('No map picture for dod_anzio');
  await expect(page.locator('.analyzer-killmap')).toHaveCount(0);
  await expect(page.locator('.analyzer-table tbody tr')).toHaveCount(2);
});

test('a POV demo explains why some deaths are missing', async ({ page }) => {
  await openKillMap(page, { demoType: 'POV' });
  await expect(page.locator('#analyzer-tab-content')).toContainText('only has positions for enemies that player could see');
});

test('an HLTV demo does not', async ({ page }) => {
  await openKillMap(page);
  await expect(page.locator('#analyzer-tab-content')).not.toContainText('only has positions for enemies');
});
