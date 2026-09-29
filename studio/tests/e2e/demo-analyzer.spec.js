// analyzer_pane.js's map picker (#217) — driven against
// tests/e2e/demo-analyzer.html. See tests/e2e/README.md for what this style of
// test covers.
import { test, expect } from '@playwright/test';

const DEMO = 'C:\\demos\\both_halves.dem';

// A report shaped like `analyze_demo_full`'s payload, with just what the
// Summary tab reads. `segments` are `demo_info.map_segments`; `mapSegment` is
// the one this report covers. Serialised into the page, so it can use nothing
// from this file's scope.
function reportFixture(segments, mapSegment, serverName) {
  return {
    file_name: 'both_halves.dem',
    file_path: 'C:\\demos\\both_halves.dem',
    file_dir: 'C:\\demos',
    file_size_mb: 84.9,
    file_created_unix_secs: 0,
    demo_info: {
      demo_protocol: 5,
      map_name: segments[0].map_name,
      network_protocol: 48,
      playback_time: 1302.6,
      playback_frames: 745542,
      game_directory: 'dod',
      demo_type: 'POV',
      map_checksum: 0,
      map_segments: segments,
    },
    state: {
      players: [],
      rounds: [],
      current_time: { viewdemo_offset: { secs: 60, nanos: 0 } },
      clan_match_detected: false,
      server_name: serverName,
      map_segment: mapSegment,
    },
  };
}

const TWO_MAPS = [
  { map_name: 'dod_lennon2', start_secs: 0, end_secs: 1288.4, start_frame: 0 },
  { map_name: 'dod_anzio', start_secs: 1288.4, end_secs: 1302.6, start_frame: 738806 },
];

async function openWith(page, segments) {
  await page.goto('/tests/e2e/demo-analyzer.html');
  await page.waitForFunction(() => window.__harnessReady === true);
  await page.evaluate(([segs, fixtureSrc]) => {
    const fixture = new Function(`return (${fixtureSrc})`)();
    // Opening a demo points the (absent) Explorer at its folder first.
    window.__mockInvokeHandlers.browse_directory = () => ({ demos: [], subdirs: [] });
    // The default analysis covers segment 0; a picked one says which it is
    // through the server name, so a test can see which report is showing.
    window.__mockInvokeHandlers.analyze_demo_full = ({ segment }) =>
      fixture(segs, segment ?? 0, segment == null ? 'default' : `segment ${segment}`);
  }, [segments, reportFixture.toString()]);
  await page.evaluate((path) => window.__openDemo(path), DEMO);
}

function analyzeCalls(page) {
  return page.evaluate(() =>
    window.__mockInvocations.filter((c) => c.cmd === 'analyze_demo_full').map((c) => c.args.segment)
  );
}

test.describe('Demo Analyzer map picker (#217)', () => {
  test('is hidden for a demo with one map', async ({ page }) => {
    await openWith(page, [TWO_MAPS[0]]);
    await expect(page.locator('.analyzer-summary-grid')).toBeVisible();
    await expect(page.locator('#analyzer-map-segment-select')).toHaveCount(0);
  });

  test('lists each map with its time range, the analysed one selected', async ({ page }) => {
    await openWith(page, TWO_MAPS);
    const picker = page.locator('#analyzer-map-segment-select');
    await expect(picker.locator('option')).toHaveText([
      'dod_lennon2 (0:00–21:28)',
      'dod_anzio (21:28–21:42)',
    ]);
    await expect(picker).toHaveValue('0');
  });

  test('picking a map re-analyses that map, and picking back asks for the default', async ({ page }) => {
    await openWith(page, TWO_MAPS);
    await page.locator('#analyzer-map-segment-select').selectOption('1');
    await expect(page.locator('.analyzer-summary-grid')).toContainText('segment 1');
    await expect(page.locator('.analyzer-summary-grid')).toContainText('dod_anzio');
    await expect(page.locator('#analyzer-map-segment-select')).toHaveValue('1');

    await page.locator('#analyzer-map-segment-select').selectOption('0');
    await expect(page.locator('.analyzer-summary-grid')).toContainText('default');
    expect(await analyzeCalls(page)).toEqual([null, 1, null]);
  });
});
