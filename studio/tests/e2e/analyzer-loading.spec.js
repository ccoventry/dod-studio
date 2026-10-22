// analyzer_pane.js — picking a second demo while the first still analyses.
//
// Both analyses keep running in the backend and both report progress. Only
// the demo picked last may show its % and its result: an earlier click's
// progress used to mix in (the % jumped around), and its result could land
// after the later one's. Driven against analyzer-multimap.html, which has
// everything the analyzer panel needs.
import { test, expect } from '@playwright/test';

const A = 'C:/demos/slow.dem';
const B = 'C:/demos/fast.dem';

const time = (secs) => ({ real_offset: { secs, nanos: 0 }, viewdemo_offset: { secs, nanos: 0 }, frame_index: 1 });

function report(fileName) {
  return {
    file_name: fileName,
    file_path: `C:/demos/${fileName}`,
    file_dir: 'C:/demos',
    file_size_mb: 80,
    file_created_unix_secs: 0,
    demo_info: {
      demo_protocol: 5, map_name: 'dod_anzio', network_protocol: 48, playback_time: 600,
      playback_frames: 1000, game_directory: 'dod', demo_type: 'POV', map_checksum: 1,
    },
    state: {
      players: [], rounds: [], chat_messages: [], team_scores: { timeline: [] },
      current_time: time(600), allies_are_british: false, clan_match_detected: false,
      signon_maps: ['dod_anzio'],
    },
  };
}

const progress = (page, path, processed) => page.evaluate(
  ({ path, processed }) => window.__mockEmit('analyzer_progress', { path, processed, total: 100 }),
  { path, processed },
);

test.beforeEach(async ({ page }) => {
  await page.goto('/tests/e2e/analyzer-multimap.html');
  await page.waitForFunction(() => window.__harnessReady === true);
  // Each analysis waits until the test releases it: window.__finish[path]().
  await page.evaluate(({ a, b }) => {
    window.__finish = {};
    window.__mockInvokeHandlers.browse_directory = () => ({ path: 'C:/demos', dirs: [], demos: [] });
    window.__mockInvokeHandlers.analyze_demo_full = ({ demoPath }) => new Promise((resolve) => {
      window.__finish[demoPath] = () => resolve(demoPath === a.file_path ? a : b);
    });
  }, { a: report('slow.dem'), b: report('fast.dem') });
});

test('only the demo picked last shows its progress and its result', async ({ page }) => {
  page.evaluate((path) => window.__openDemo(path), A);
  await page.waitForFunction((p) => !!window.__finish[p], A);
  page.evaluate((path) => window.__openDemo(path), B);
  await page.waitForFunction((p) => !!window.__finish[p], B);

  const content = page.locator('#analyzer-tab-content');
  await progress(page, B, 30);
  await expect(content).toHaveText('Analyzing demo… 30%');
  await progress(page, A, 80);
  await expect(content).toHaveText('Analyzing demo… 30%');
  await progress(page, B, 45);
  await expect(content).toHaveText('Analyzing demo… 45%');

  await page.evaluate((p) => window.__finish[p](), B);
  await expect(page.locator('#analyzer-current-file')).toHaveText('fast.dem');
  // The first click's analysis finishing later doesn't replace it.
  await page.evaluate((p) => window.__finish[p](), A);
  await page.waitForTimeout(100);
  await expect(page.locator('#analyzer-current-file')).toHaveText('fast.dem');
});

test('clicking a demo that is still analysing does not start it again', async ({ page }) => {
  page.evaluate((path) => window.__openDemo(path), A);
  await page.waitForFunction((p) => !!window.__finish[p], A);
  page.evaluate((path) => window.__openDemo(path), A);
  await page.waitForTimeout(100);
  const runs = await page.evaluate(() => window.__mockInvocations.filter((c) => c.cmd === 'analyze_demo_full').length);
  expect(runs).toBe(1);
  await page.evaluate((p) => window.__finish[p](), A);
  await expect(page.locator('#analyzer-current-file')).toHaveText('slow.dem');
});
