// analyzer_protocol47.js — the Demo Analyzer's protocol-47 notice (#239):
// a demo recorded with protocol 47 reads fine but the game refuses to play
// it, so the notice offers Convert, which writes <name>_p48.dem beside it
// and opens that copy. Uses the multi-map harness, which has the banner.
import { test, expect } from './fixtures.js';

const DEMO = 'C:/demos/donner-vtec-dice1.dem';
const COPY = 'C:/demos/donner-vtec-dice1_p48.dem';

const time = (secs) => ({ real_offset: { secs, nanos: 0 }, viewdemo_offset: { secs, nanos: 0 }, frame_index: 1 });

function report(fileName, networkProtocol) {
  return {
    file_name: fileName,
    file_path: `C:/demos/${fileName}`,
    file_dir: 'C:/demos',
    file_size_mb: 40,
    file_created_unix_secs: 0,
    demo_info: {
      demo_protocol: 5, map_name: 'dod_donner', network_protocol: networkProtocol, playback_time: 1300,
      playback_frames: 1000, game_directory: 'dod', demo_type: 'HLTV', map_checksum: 1, damage: null,
    },
    state: {
      players: [], rounds: [], chat_messages: [], team_scores: { timeline: [] },
      current_time: time(1300), allies_are_british: false, clan_match_detected: false, signon_maps: ['dod_donner'],
    },
  };
}

/** Opens DEMO (protocol 47) with a convert_protocol_47 that waits for
 *  window.__finishConvert(), and resolves COPY or rejects with `fails`. */
async function open(page, { fails = null } = {}) {
  await page.evaluate(({ old, copy, fails, written }) => {
    window.__mockInvokeHandlers.analyze_demo_full = ({ demoPath }) => (demoPath.endsWith('_p48.dem') ? copy : old);
    window.__mockInvokeHandlers.browse_directory = () => ({ path: 'C:/demos', dirs: [], demos: [] });
    window.__mockInvokeHandlers.convert_protocol_47 = ({ path }) => new Promise((resolve, reject) => {
      window.__convertPath = path;
      window.__finishConvert = () => (fails ? reject(fails) : resolve(written));
    });
  }, {
    old: report('donner-vtec-dice1.dem', 47),
    copy: report('donner-vtec-dice1_p48.dem', 48),
    fails,
    written: COPY,
  });
  await page.evaluate((path) => window.__openDemo(path), DEMO);
}

test.beforeEach(async ({ page }) => {
  await page.goto('/tests/e2e/analyzer-multimap.html');
  await page.waitForFunction(() => window.__harnessReady === true);
});

test('a protocol-47 demo gets the notice; Convert says it is working, then opens the copy', async ({ page }) => {
  await open(page);
  const banner = page.locator('#analyzer-protocol47-banner');
  await expect(banner).toBeVisible();
  await expect(banner).toContainText('Protocol 47 demo');
  const btn = banner.locator('.analyzer-protocol47-convert');
  await expect(btn).toHaveText('Convert');
  await btn.click();
  await expect(btn).toBeDisabled();
  await expect(banner.locator('.analyzer-protocol47-status')).toHaveText('Converting…');
  await page.waitForFunction(() => typeof window.__finishConvert === 'function');
  expect(await page.evaluate(() => window.__convertPath)).toBe(DEMO);
  await page.evaluate(() => window.__finishConvert());
  await expect(page.locator('#analyzer-current-file')).toHaveText('donner-vtec-dice1_p48.dem');
  await expect(banner).toBeHidden();
  await expect(page.locator('#toast-container')).toContainText('Converted: donner-vtec-dice1_p48.dem');
});

test('a failed conversion says why and leaves Convert usable', async ({ page }) => {
  await open(page, { fails: 'the disk is full' });
  const banner = page.locator('#analyzer-protocol47-banner');
  await banner.locator('.analyzer-protocol47-convert').click();
  await page.waitForFunction(() => typeof window.__finishConvert === 'function');
  await page.evaluate(() => window.__finishConvert());
  await expect(banner.locator('.analyzer-protocol47-status')).toHaveText("Couldn't convert it: the disk is full");
  await expect(banner.locator('.analyzer-protocol47-convert')).toBeEnabled();
  await expect(page.locator('#analyzer-current-file')).toHaveText('donner-vtec-dice1.dem');
});

test('a protocol-48 demo shows no notice', async ({ page }) => {
  await page.evaluate((plain) => {
    window.__mockInvokeHandlers.analyze_demo_full = () => plain;
    window.__mockInvokeHandlers.browse_directory = () => ({ path: 'C:/demos', dirs: [], demos: [] });
  }, report('anzio-new.dem', 48));
  await page.evaluate(() => window.__openDemo('C:/demos/anzio-new.dem'));
  await expect(page.locator('#analyzer-current-file')).toHaveText('anzio-new.dem');
  await expect(page.locator('#analyzer-protocol47-banner')).toBeHidden();
});
