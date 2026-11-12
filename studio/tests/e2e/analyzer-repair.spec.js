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

test('a damaged demo with more than one map greys out Split now and offers Repair & Split', async ({ page }) => {
  const both = report('monday-wsod25_r07_m1_h2.dem', DAMAGE);
  both.state.signon_maps = ['dod_anzio', 'dod_pandemic_aim'];
  const piece = report('monday-wsod25_r07_m1_h2_repaired_split1_anzio.dem', null);
  await page.evaluate(({ both, piece }) => {
    window.__mockInvokeHandlers.analyze_demo_full = ({ demoPath }) => (demoPath.includes('_split1_') ? piece : both);
    window.__mockInvokeHandlers.browse_directory = () => ({ path: 'C:/demos', dirs: [], demos: [] });
    window.__mockInvokeHandlers.demo_split_status = () => ({ split: false, files: [] });
    window.__mockInvokeHandlers.repair_demo_cmd = () => ({
      verdict: 'repaired', written: 'C:/demos/monday-wsod25_r07_m1_h2_repaired.dem',
      holes: [{ before_secs: 1, after_secs: 2, lost_secs: 1, bytes_skipped: 1 }, { before_secs: 3, after_secs: 4, lost_secs: 1, bytes_skipped: 1 }],
      seconds_lost: 2, percent_kept: 97.7, ends_at_last_hole: false, reasons: [],
    });
    window.__mockInvokeHandlers.split_demo_auto = ({ path }) => new Promise((resolve) => {
      window.__splitPath = path;
      window.__finishSplit = () => resolve([
        { index: 0, path: 'C:/demos/monday-wsod25_r07_m1_h2_repaired_split1_anzio.dem', map: 'dod_anzio', seconds: 1479, size_bytes: 1 },
        { index: 1, path: 'C:/demos/monday-wsod25_r07_m1_h2_repaired_split2_pandemic_aim.dem', map: 'dod_pandemic_aim', seconds: 48, size_bytes: 1 },
      ]);
    });
  }, { both, piece });
  await page.evaluate(() => window.__openDemo('C:/demos/monday-wsod25_r07_m1_h2.dem'));

  const split = page.locator('#analyzer-multimap-banner .analyzer-multimap-split');
  await expect(split).toBeDisabled();
  await expect(split).toHaveAttribute('title', /Repair it first/);
  const banner = page.locator('#analyzer-damage-banner');
  await banner.locator('.analyzer-damage-repair-split').click();
  await page.waitForFunction(() => typeof window.__finishSplit === 'function');
  expect(await page.evaluate(() => window.__splitPath)).toBe('C:/demos/monday-wsod25_r07_m1_h2_repaired.dem');
  await page.evaluate(() => window.__mockEmit('split_progress', {
    path: 'C:/demos/monday-wsod25_r07_m1_h2_repaired.dem', progress: { fraction: 0.5, stage: 'writing', map: 'dod_anzio', part: 1, parts: 2 },
  }));
  await expect(banner.locator('.split-progress-text')).toHaveText('Splitting the repaired demo: Writing dod_anzio (1 of 2)… 50%');
  await expect(banner.locator('.progress-bar-fill')).toHaveAttribute('style', /width: 75%/);
  await page.evaluate(() => window.__finishSplit());
  await expect(page.locator('#analyzer-current-file')).toHaveText('monday-wsod25_r07_m1_h2_repaired_split1_anzio.dem');
  await expect(page.locator('#toast-container')).toContainText('Repaired (2 gaps cut out) and split into 2 demos');
});

/** A damaged two-map demo, with `repairable` as demo_repairable's answer and
 *  `verdict` as the repair's. */
async function openTwoMaps(page, { repairable, verdict = 'not_damaged' }) {
  const both = report('monday-wsod25_r07_m1_h2.dem', DAMAGE);
  both.state.signon_maps = ['dod_anzio', 'dod_pandemic_aim'];
  await page.evaluate(({ both, repairable, verdict }) => {
    window.__mockInvokeHandlers.analyze_demo_full = () => both;
    window.__mockInvokeHandlers.browse_directory = () => ({ path: 'C:/demos', dirs: [], demos: [] });
    window.__mockInvokeHandlers.demo_split_status = () => ({ split: false, files: [] });
    window.__mockInvokeHandlers.demo_repairable = () => ({ repairable, maps: both.state.signon_maps });
    window.__mockInvokeHandlers.repair_demo_cmd = () => ({ verdict, holes: [], reasons: [] });
  }, { both, repairable, verdict });
  await page.evaluate(() => window.__openDemo('C:/demos/monday-wsod25_r07_m1_h2.dem'));
}

test('damage the repair finds no break in keeps its notice, offers no Repair, and Split now says why it is greyed out', async ({ page }) => {
  await openTwoMaps(page, { repairable: false });
  const banner = page.locator('#analyzer-damage-banner');
  await expect(banner).toContainText('Damaged demo: read up to 8:27.');
  await expect(banner).toContainText("Repair can't fix this one");
  await expect(banner.locator('button')).toHaveCount(0);
  const split = page.locator('#analyzer-multimap-banner .analyzer-multimap-split');
  await expect(split).toBeDisabled();
  await expect(split).toHaveAttribute('title', "The damage stops a split, and Repair can't cut it out.");
});

test("a Repair that finds nothing to cut out turns the notice and Split now's reason", async ({ page }) => {
  await openTwoMaps(page, { repairable: true });
  const banner = page.locator('#analyzer-damage-banner');
  const split = page.locator('#analyzer-multimap-banner .analyzer-multimap-split');
  await expect(split).toHaveAttribute('title', /Repair it first/);
  await banner.locator('.analyzer-damage-repair').click();
  await expect(banner).toContainText("Repair can't fix this one");
  await expect(banner.locator('button')).toHaveCount(0);
  await expect(split).toBeDisabled();
  await expect(split).toHaveAttribute('title', "The damage stops a split, and Repair can't cut it out.");
});

test('a demo damaged before its level change offers Repair & Split once the repair check finds its maps', async ({ page }) => {
  // The analysis stops at the damage, before the level change: one map.
  const early = report('monday-wsod25_r07_m1_h2.dem', DAMAGE);
  await page.evaluate(({ early }) => {
    window.__mockInvokeHandlers.analyze_demo_full = () => early;
    window.__mockInvokeHandlers.browse_directory = () => ({ path: 'C:/demos', dirs: [], demos: [] });
    window.__mockInvokeHandlers.demo_split_status = () => ({ split: false, files: [] });
    window.__mockInvokeHandlers.demo_repairable = () => ({ repairable: true, maps: ['dod_anzio', 'dod_pandemic_aim', 'dod_thunder2_b5a'] });
  }, { early });
  await page.evaluate(() => window.__openDemo('C:/demos/monday-wsod25_r07_m1_h2.dem'));
  const banner = page.locator('#analyzer-damage-banner');
  await expect(banner.locator('button')).toHaveText(['Repair now', 'Repair & Split']);
  // Its own multi-map notice stays as it was: the analysis knows one map.
  await expect(page.locator('#analyzer-multimap-banner')).toBeHidden();
});

test('a one-map damaged demo the repair check finds no more maps in keeps Repair now alone', async ({ page }) => {
  const one = report('monday-wsod25_r07_m1_h2.dem', DAMAGE);
  await page.evaluate(({ one }) => {
    window.__mockInvokeHandlers.analyze_demo_full = () => one;
    window.__mockInvokeHandlers.browse_directory = () => ({ path: 'C:/demos', dirs: [], demos: [] });
    window.__mockInvokeHandlers.demo_repairable = () => ({ repairable: true, maps: ['dod_anzio'] });
  }, { one });
  await page.evaluate(() => window.__openDemo('C:/demos/monday-wsod25_r07_m1_h2.dem'));
  const banner = page.locator('#analyzer-damage-banner');
  await expect(banner.locator('.analyzer-damage-repair')).toBeVisible();
  await expect(banner.locator('button')).toHaveCount(1);
});

test('a demo damaged before its level change whose repair was already split offers no repair', async ({ page }) => {
  // The live repro: m1_h2 repaired and split from the queue, then the
  // original opened in the Demo Analyzer. Its analysis knows one map; the
  // "Already split" check must ask with the repair check's three.
  const early = report('monday-wsod25_r07_m1_h2.dem', DAMAGE);
  const maps = ['dod_anzio', 'dod_pandemic_aim', 'dod_thunder2_b5a'];
  await page.evaluate(({ early, maps }) => {
    window.__mockInvokeHandlers.analyze_demo_full = () => early;
    window.__mockInvokeHandlers.browse_directory = () => ({ path: 'C:/demos', dirs: [], demos: [] });
    window.__mockInvokeHandlers.demo_repairable = () => ({ repairable: true, maps });
    window.__mockInvokeHandlers.demo_split_status = ({ maps: asked }) => {
      window.__splitAskedWith = asked;
      return {
        split: true,
        files: ['monday-wsod25_r07_m1_h2_repaired_split1_anzio.dem', 'monday-wsod25_r07_m1_h2_repaired_split3_thunder2_b5a.dem'],
      };
    };
  }, { early, maps });
  await page.evaluate(() => window.__openDemo('C:/demos/monday-wsod25_r07_m1_h2.dem'));
  const banner = page.locator('#analyzer-damage-banner');
  await expect(banner).toContainText('Damaged demo: read up to 8:27.');
  await expect(banner).toContainText('Already repaired and split into its 3 maps (dod_anzio, dod_pandemic_aim, dod_thunder2_b5a) next to it.');
  await expect(banner.locator('button')).toHaveCount(0);
  await expect(banner.locator('.analyzer-damage-already-split')).toHaveAttribute('title', /monday-wsod25_r07_m1_h2_repaired_split1_anzio\.dem/);
  expect(await page.evaluate(() => window.__splitAskedWith)).toEqual(maps);
  await expect(page.locator('#analyzer-multimap-banner')).toBeHidden();
});

test('a damaged two-map demo whose repair was already split offers no repair, and the multi-map notice says split', async ({ page }) => {
  const both = report('monday-wsod25_r07_m1_h2.dem', DAMAGE);
  both.state.signon_maps = ['dod_anzio', 'dod_pandemic_aim'];
  await page.evaluate(({ both }) => {
    window.__mockInvokeHandlers.analyze_demo_full = () => both;
    window.__mockInvokeHandlers.browse_directory = () => ({ path: 'C:/demos', dirs: [], demos: [] });
    window.__mockInvokeHandlers.demo_repairable = () => ({ repairable: true, maps: both.state.signon_maps });
    window.__mockInvokeHandlers.demo_split_status = () => ({ split: true, files: ['a_repaired_split1_anzio.dem', 'a_repaired_split2_pandemic_aim.dem'] });
  }, { both });
  await page.evaluate(() => window.__openDemo('C:/demos/monday-wsod25_r07_m1_h2.dem'));
  const banner = page.locator('#analyzer-damage-banner');
  await expect(banner.locator('button')).toHaveCount(0);
  // Said once, by the multi-map notice, not again in the damage notice.
  await expect(banner.locator('.analyzer-damage-already-split')).toHaveCount(0);
  await expect(page.locator('#analyzer-multimap-banner')).toContainText('already split');
});
