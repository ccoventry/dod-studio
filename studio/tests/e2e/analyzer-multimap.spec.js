// analyzer_pane.js — a demo that recorded more than one map (#217).
//
// The analyzer covers only one map of such a demo, so a notice above the tabs
// says so and offers Split now: it asks for the maps, splits off every one
// longer than a minute, and opens the first demo written. Which maps it keeps
// is unit-tested in src/analyzer_multimap.test.js.
import { test, expect } from '@playwright/test';

const DEMO = 'C:/demos/wsod25_grp3_h1_dyelife.dem';
const FIRST = 'C:/demos/wsod25_grp3_h1_dyelife_dod_lennon2_1.dem';

const time = (secs) => ({ real_offset: { secs, nanos: 0 }, viewdemo_offset: { secs, nanos: 0 }, frame_index: 1 });

/** native's AnalyzerReportPayload, cut to what the Summary tab reads. */
function report(fileName, signonMaps) {
  return {
    file_name: fileName,
    file_path: `C:/demos/${fileName}`,
    file_dir: 'C:/demos',
    file_size_mb: 85,
    file_created_unix_secs: 0,
    demo_info: {
      demo_protocol: 5, map_name: 'dod_lennon2', network_protocol: 48, playback_time: 1300,
      playback_frames: 1000, game_directory: 'dod', demo_type: 'POV', map_checksum: 1,
    },
    state: {
      players: [],
      rounds: [],
      chat_messages: [],
      team_scores: { timeline: [] },
      current_time: time(1288),
      allies_are_british: false,
      clan_match_detected: false,
      signon_maps: signonMaps,
    },
  };
}

const SEGMENTS = [
  { index: 0, map: 'dod_lennon2', map_checksum: 1, start_seconds: -2, end_seconds: 1288, frames: 90000 },
  { index: 1, map: 'dod_lennon2', map_checksum: 1, start_seconds: 1288, end_seconds: 1302, frames: 900 },
];

async function open(page, signonMaps, { splitFails = false } = {}) {
  await page.evaluate(({ two, one, splitFails, first }) => {
    window.__mockInvokeHandlers.analyze_demo_full = ({ demoPath }) => (demoPath.endsWith('_1.dem') ? one : two);
    window.__mockInvokeHandlers.browse_directory = () => ({ path: 'C:/demos', dirs: [], demos: [] });
    window.__mockInvokeHandlers.demo_map_segments = () => window.__segments;
    window.__mockInvokeHandlers.split_demo_maps = () => (splitFails
      ? Promise.reject('the disk is full')
      : [{ path: first, map: 'dod_lennon2', seconds: 1290, size_bytes: 84000000 }]);
  }, {
    two: report('wsod25_grp3_h1_dyelife.dem', signonMaps),
    one: report('wsod25_grp3_h1_dyelife_dod_lennon2_1.dem', ['dod_lennon2']),
    splitFails,
    first: FIRST,
  });
  await page.evaluate((s) => { window.__segments = s; }, SEGMENTS);
  await page.evaluate((path) => window.__openDemo(path), DEMO);
}

test.beforeEach(async ({ page }) => {
  await page.goto('/tests/e2e/analyzer-multimap.html');
  await page.waitForFunction(() => window.__harnessReady === true);
});

test('a demo with two maps says so above the tabs', async ({ page }) => {
  await open(page, ['dod_lennon2', 'dod_lennon2']);
  const banner = page.locator('#analyzer-multimap-banner');
  await expect(banner).toBeVisible();
  await expect(banner).toContainText('This demo recorded 2 maps (dod_lennon2, dod_lennon2).');
  await expect(banner).toContainText('Only one is analysed');
  await expect(banner.locator('button')).toHaveText('Split now');
});

test('a one-map demo, or an older cache entry with no map list, shows nothing', async ({ page }) => {
  await open(page, ['dod_lennon2']);
  await expect(page.locator('#analyzer-multimap-banner')).toBeHidden();
  await open(page, undefined);
  await expect(page.locator('#analyzer-multimap-banner')).toBeHidden();
});

test('Split now splits off the long maps, then opens the first demo written', async ({ page }) => {
  await open(page, ['dod_lennon2', 'dod_lennon2']);
  await page.locator('#analyzer-multimap-banner button').click();

  // The 14 s stub of the second half is left out.
  await expect.poll(() => page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'split_demo_maps')?.args))
    .toEqual({ path: DEMO, keep: [0] });
  await expect(page.locator('#analyzer-current-file')).toHaveText('wsod25_grp3_h1_dyelife_dod_lennon2_1.dem');
  await expect(page.locator('#analyzer-multimap-banner')).toBeHidden();
  await expect(page.locator('#toast-container')).toContainText('Split into 1 demo: wsod25_grp3_h1_dyelife_dod_lennon2_1.dem');
});

test('a failed split says why and leaves the button usable', async ({ page }) => {
  await open(page, ['dod_lennon2', 'dod_lennon2'], { splitFails: true });
  const banner = page.locator('#analyzer-multimap-banner');
  await banner.locator('button').click();
  await expect(banner).toContainText('Could not split it: the disk is full');
  await expect(banner.locator('button')).toBeEnabled();
  await expect(page.locator('#analyzer-current-file')).toHaveText('wsod25_grp3_h1_dyelife.dem');
});
