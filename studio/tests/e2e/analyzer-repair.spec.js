// analyzer_damage.js + analyzer_repair.js — the Demo Analyzer's damaged-demo
// notice (#811) offers Repair now (#15): it repairs the demo into
// <name>_repaired.dem beside it, shows the repair's progress, and opens the
// repaired demo. Uses the multi-map harness, which has the damage banner.
import { test, expect } from '@playwright/test';

const DEMO = 'C:/demos/monday-wsod25_r07_m1_h1.dem';
const REPAIRED = 'C:/demos/monday-wsod25_r07_m1_h1_repaired.dem';

const time = (secs) => ({ real_offset: { secs, nanos: 0 }, viewdemo_offset: { secs, nanos: 0 }, frame_index: 1 });

function report(fileName, damage) {
  return {
    file_name: fileName,
    file_path: `C:/demos/${fileName}`,
    file_dir: 'C:/demos',
    file_size_mb: 82,
    file_created_unix_secs: 0,
    demo_info: {
      demo_protocol: 5, map_name: 'dod_anzio', network_protocol: 48, playback_time: 1300,
      playback_frames: 1000, game_directory: 'dod', demo_type: 'POV', map_checksum: 1, damage,
    },
    state: {
      players: [], rounds: [], chat_messages: [], team_scores: { timeline: [] },
      current_time: time(507), allies_are_british: false, clan_match_detected: false, signon_maps: ['dod_anzio'],
    },
  };
}

const DAMAGE = { read_up_to_secs: 507.4, frame_index: 14846, byte_offset: 2433784, bytes_unread: 79927172, reason: 'a frame does not parse (Eof)' };

async function open(page, { fails = false } = {}) {
  await page.evaluate(({ damaged, whole, fails, repaired }) => {
    window.__mockInvokeHandlers.analyze_demo_full = ({ demoPath }) => (demoPath.endsWith('_repaired.dem') ? whole : damaged);
    window.__mockInvokeHandlers.browse_directory = () => ({ path: 'C:/demos', dirs: [], demos: [] });
    window.__mockInvokeHandlers.repair_demo_cmd = () => new Promise((resolve, reject) => {
      window.__finishRepair = () => (fails
        ? reject('the disk is full')
        : resolve({
          verdict: 'repaired', written: repaired, holes: [{ before_secs: 39.2, after_secs: 68.6, lost_secs: 29.4, bytes_skipped: 1 }],
          seconds_lost: 29.4, percent_kept: 96.7, ends_at_last_hole: false, reasons: [],
        }));
    });
  }, {
    damaged: report('monday-wsod25_r07_m1_h1.dem', DAMAGE),
    whole: report('monday-wsod25_r07_m1_h1_repaired.dem', null),
    fails,
    repaired: REPAIRED,
  });
  await page.evaluate((path) => window.__openDemo(path), DEMO);
}

test.beforeEach(async ({ page }) => {
  await page.goto('/tests/e2e/analyzer-multimap.html');
  await page.waitForFunction(() => window.__harnessReady === true);
});

test('the damaged-demo notice offers Repair now, which shows its progress and opens the repair', async ({ page }) => {
  await open(page);
  const banner = page.locator('#analyzer-damage-banner');
  await expect(banner).toContainText('Damaged demo: read up to 8:27.');
  const btn = banner.locator('.analyzer-damage-repair');
  await expect(btn).toHaveText('Repair now');
  await btn.click();
  await expect(btn).toBeDisabled();
  await expect(banner.locator('.split-progress-text')).toHaveText('Finding the gaps… 0%');
  await page.evaluate((path) => window.__mockEmit('repair_progress', { path, progress: { step: 'checking', fraction: 0.8 } }), DEMO);
  await expect(banner.locator('.split-progress-text')).toHaveText('Checking the repaired demo… 80%');
  await page.evaluate(() => window.__finishRepair());
  await expect(page.locator('#analyzer-current-file')).toHaveText('monday-wsod25_r07_m1_h1_repaired.dem');
  await expect(banner).toBeHidden();
  await expect(page.locator('#toast-container')).toContainText('Repaired: 1 gap cut out');
});

test('a failed repair says why and leaves the button usable', async ({ page }) => {
  await open(page, { fails: true });
  const banner = page.locator('#analyzer-damage-banner');
  await banner.locator('.analyzer-damage-repair').click();
  await page.waitForFunction(() => typeof window.__finishRepair === 'function');
  await page.evaluate(() => window.__finishRepair());
  await expect(banner).toContainText('Repair failed: the disk is full');
  await expect(banner.locator('.analyzer-damage-repair')).toBeEnabled();
  await expect(page.locator('#analyzer-current-file')).toHaveText('monday-wsod25_r07_m1_h1.dem');
});
