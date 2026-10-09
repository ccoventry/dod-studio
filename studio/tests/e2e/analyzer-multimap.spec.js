// analyzer_pane.js — a demo that recorded more than one map (#217).
//
// The analyzer covers only the first map of such a demo, so a notice above
// the tabs says so and offers Split now: native splits off every map of a
// minute or more (split_demo_auto; which maps is native's keep_at_least,
// tested there), the notice shows its progress, and the first demo written
// opens.
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

/**
 * Opens DEMO. `split_demo_auto` waits until the test calls
 * window.__finishSplit(), so the progress in between can be checked; with
 * `splitFails` it rejects instead.
 */
async function open(page, signonMaps, { splitFails = false } = {}) {
  await page.evaluate(({ two, one, splitFails, first }) => {
    window.__mockInvokeHandlers.analyze_demo_full = ({ demoPath }) => (demoPath.endsWith('_1.dem') ? one : two);
    window.__mockInvokeHandlers.browse_directory = () => ({ path: 'C:/demos', dirs: [], demos: [] });
    window.__mockInvokeHandlers.split_demo_auto = () => new Promise((resolve, reject) => {
      window.__finishSplit = () => (splitFails
        ? reject('the disk is full')
        : resolve([{ path: first, map: 'dod_lennon2', seconds: 1290, size_bytes: 84000000 }]));
    });
  }, {
    two: report('wsod25_grp3_h1_dyelife.dem', signonMaps),
    one: report('wsod25_grp3_h1_dyelife_dod_lennon2_1.dem', ['dod_lennon2']),
    splitFails,
    first: FIRST,
  });
  await page.evaluate((path) => window.__openDemo(path), DEMO);
}

const progress = (page, path, p) => page.evaluate(({ path, p }) => window.__mockEmit('split_progress', { path, progress: p }), { path, p });

test.beforeEach(async ({ page }) => {
  await page.goto('/tests/e2e/analyzer-multimap.html');
  await page.waitForFunction(() => window.__harnessReady === true);
});

test('a demo with two maps says so above the tabs', async ({ page }) => {
  await open(page, ['dod_lennon2', 'dod_lennon2']);
  const banner = page.locator('#analyzer-multimap-banner');
  await expect(banner).toBeVisible();
  await expect(banner).toContainText('This demo recorded 2 maps (dod_lennon2, dod_lennon2).');
  await expect(banner).toContainText('Only the first is analysed');
  await expect(banner.locator('button')).toHaveText('Split now');
});

test('a one-map demo, or an older cache entry with no map list, shows nothing', async ({ page }) => {
  await open(page, ['dod_lennon2']);
  await expect(page.locator('#analyzer-multimap-banner')).toBeHidden();
  await open(page, undefined);
  await expect(page.locator('#analyzer-multimap-banner')).toBeHidden();
});

test('Split now shows its progress at once and as native reports it', async ({ page }) => {
  await open(page, ['dod_lennon2', 'dod_lennon2']);
  const banner = page.locator('#analyzer-multimap-banner');
  await banner.locator('button').click();
  await expect(banner.locator('button')).toBeDisabled();
  await expect(banner.locator('.split-progress-text')).toHaveText('Reading the demo… 0%');

  await progress(page, DEMO, { fraction: 0.2, stage: 'reading', map: '', part: 0, parts: 0 });
  await expect(banner.locator('.split-progress-text')).toHaveText('Reading the demo… 20%');
  await expect(banner.locator('.progress-bar-fill')).toHaveAttribute('style', /width: 20%/);
  await progress(page, DEMO, { fraction: 0.55, stage: 'writing', map: 'dod_lennon2', part: 1, parts: 1 });
  await expect(banner.locator('.split-progress-text')).toHaveText('Writing dod_lennon2… 55%');
  // Another demo's split is not this one's.
  await progress(page, 'C:/demos/other.dem', { fraction: 0.9, stage: 'checking', map: 'dod_anzio', part: 1, parts: 1 });
  await expect(banner.locator('.split-progress-text')).toHaveText('Writing dod_lennon2… 55%');
});

test('Split now splits off the long maps, then opens the first demo written', async ({ page }) => {
  await open(page, ['dod_lennon2', 'dod_lennon2']);
  await page.locator('#analyzer-multimap-banner button').click();
  await expect.poll(() => page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'split_demo_auto')?.args))
    .toEqual({ path: DEMO, minSeconds: 60 });
  await page.evaluate(() => window.__finishSplit());

  await expect(page.locator('#analyzer-current-file')).toHaveText('wsod25_grp3_h1_dyelife_dod_lennon2_1.dem');
  await expect(page.locator('#analyzer-multimap-banner')).toBeHidden();
  await expect(page.locator('#toast-container')).toContainText('Split into 1 demo: wsod25_grp3_h1_dyelife_dod_lennon2_1.dem');
});

test('a failed split says why and leaves the button usable', async ({ page }) => {
  await open(page, ['dod_lennon2', 'dod_lennon2'], { splitFails: true });
  const banner = page.locator('#analyzer-multimap-banner');
  await banner.locator('button').click();
  await page.waitForFunction(() => typeof window.__finishSplit === 'function');
  await page.evaluate(() => window.__finishSplit());
  await expect(banner).toContainText('Could not split it: the disk is full');
  await expect(banner.locator('.split-progress')).toHaveCount(0);
  await expect(banner.locator('button')).toBeEnabled();
  await expect(page.locator('#analyzer-current-file')).toHaveText('wsod25_grp3_h1_dyelife.dem');
});
