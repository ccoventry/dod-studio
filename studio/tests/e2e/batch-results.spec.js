// batch_results.js — the Last Batch panel (#172).
import { test, expect } from '@playwright/test';

const streak = (player, kills, at) => ({ target_player: player, kill_count: kills, viewdemo_times: [at], kills: [[0, at, 'garand']], start_index: 0, end_index: 0 });
const DISPATCH = { streaks: [streak('krod', 4, 754), streak('krod', 2, 800), streak('milo', 3, 65)] };
const PAYLOAD = {
  total_count: 3, captured_count: 2, renderable_count: 1,
  blocks: [
    { take_key: 'a', demo_name: 'anzio.dem', source_streak_indices: [0, 1], captured: true, renderable: true, bytes: 1.5 * 1024 ** 3, take_folder: 'C:\caps\a' },
    { take_key: 'b', demo_name: 'flash.dem', source_streak_indices: [2], captured: true, renderable: false, bytes: 300 * 1024 ** 2, take_folder: 'C:\caps\b' },
    { take_key: 'c', demo_name: 'kalt.dem', source_streak_indices: [], captured: false, renderable: false, bytes: 0, take_folder: 'C:\caps\c' },
  ],
};

async function loadHarness(page) {
  await page.goto('/tests/e2e/batch-results.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

test('a finished batch lists each take: what it was, whether it landed, its size', async ({ page }) => {
  await loadHarness(page);
  await expect(page.locator('#batch-results')).toBeHidden();
  await page.evaluate(([p, d]) => { window.__ended('completed', ''); window.__verified(p, d); }, [PAYLOAD, DISPATCH]);
  const panel = page.locator('#batch-results');
  await expect(panel).toBeVisible();
  await expect(panel.locator('.batch-results-summary')).toHaveText(
    "Completed. 2 of 3 takes on disk, 1.79 GB; 1 Render Studio can't use yet.");
  const rows = panel.locator('tbody tr');
  await expect(rows).toHaveCount(3);
  await expect(rows.nth(0)).toContainText('Captured');
  await expect(rows.nth(0)).toContainText('anzio.dem · krod · 4 kills · 12:34 (+1 more, recorded as one take)');
  await expect(rows.nth(0)).toContainText('1.50 GB');
  await expect(rows.nth(1)).toContainText("Captured, can't render yet");
  await expect(rows.nth(1)).toContainText('300.0 MB');
  await expect(rows.nth(2)).toContainText('Missing');
  await expect(rows.nth(2).getByRole('button')).toHaveCount(0);
  await rows.nth(0).getByRole('button', { name: 'Open folder' }).click();
  const opened = await page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'reveal_in_explorer')?.args.path);
  expect(opened).toBe('C:\caps\a');
});

test('the takes can arrive before the outcome, an error says why, and the next batch clears it', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(([p, d]) => window.__verified(p, d), [PAYLOAD, DISPATCH]);
  await expect(page.locator('.batch-results-summary')).toHaveText(/^2 of 3 takes on disk/);
  await page.evaluate(() => window.__ended('error', 'The game crashed on dod_kalt'));
  await expect(page.locator('.batch-results-summary')).toHaveText(/^Stopped: The game crashed on dod_kalt\. 2 of 3/);
  await expect(page.locator('.batch-results-summary')).toHaveClass(/batch-results-error/);
  await page.evaluate(() => window.__started());
  await expect(page.locator('#batch-results')).toBeHidden();
});

test('an OBS batch shows what OBS dropped per take and names the fix (#172)', async ({ page }) => {
  await loadHarness(page);
  const clip = (n, folder, skipped, lagging) => ({
    clip: n, take_folder: folder, recording: false, skipped_pct: skipped, missed_pct: 0,
    skipped: Math.round(skipped * 12), frames: 1200, missed: 0, rendered: 1200, lagging, capture_fps: 120,
  });
  const [a, b] = PAYLOAD.blocks;
  await page.evaluate(([p, d, x, y]) => {
    window.__obsClip(x);
    window.__obsClip(y);
    window.__ended('completed', '');
    window.__verified(p, d);
  }, [PAYLOAD, DISPATCH, clip(1, a.take_folder, 34.2, true), clip(2, b.take_folder, 0.3, false)]);
  const panel = page.locator('#batch-results');
  await expect(panel.locator('.batch-results-summary')).toContainText(
    'OBS dropped frames in 1 of 2 takes. Lower OBS Capture FPS to 60');
  const rows = panel.locator('tbody tr');
  await expect(rows.nth(0)).toContainText('OBS: 34.2% skipped');
  await expect(rows.nth(0).locator('.batch-results-obs-lagging')).toHaveCount(1);
  await expect(rows.nth(1)).toContainText('OBS: 0.3% skipped');
  await expect(rows.nth(1).locator('.batch-results-obs-lagging')).toHaveCount(0);
  await page.evaluate(() => window.__started());
  await page.evaluate(([p, d]) => { window.__ended('completed', ''); window.__verified(p, d); }, [PAYLOAD, DISPATCH]);
  await expect(panel.locator('.batch-results-summary')).not.toContainText('OBS');
});

test('the panel can be hidden until the next batch', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => window.__ended('cancelled', ''));
  await expect(page.locator('.batch-results-summary')).toHaveText('Cancelled. Checking the takes on disk…');
  await page.click('.batch-results-close');
  await expect(page.locator('#batch-results')).toBeHidden();
});

test('a clip Cancel cut short shows as a partial take, not a captured one (#1081)', async ({ page }) => {
  await loadHarness(page);
  const payload = {
    total_count: 2, captured_count: 1, renderable_count: 1,
    blocks: [
      { ...PAYLOAD.blocks[0] },
      { take_key: 'b', demo_name: 'flash.dem', source_streak_indices: [2], captured: false, renderable: false, cut_short: true, bytes: 200 * 1024 ** 2, take_folder: 'C:/caps/b' },
    ],
  };
  await page.evaluate(([p, d]) => { window.__ended('cancelled', ''); window.__verified(p, d); }, [payload, DISPATCH]);
  await expect(page.locator('.batch-results-summary')).toHaveText(/^Cancelled\. 1 of 2 takes on disk/);
  const row = page.locator('#batch-results tbody tr').nth(1);
  await expect(row).toContainText('Cut short by Cancel (partial take, not captured)');
  await expect(row).toContainText('200.0 MB');
  await expect(row.getByRole('button', { name: 'Open folder' })).toHaveCount(1);
});
