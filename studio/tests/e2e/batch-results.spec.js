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

test('the panel can be hidden until the next batch', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => window.__ended('cancelled', ''));
  await expect(page.locator('.batch-results-summary')).toHaveText('Cancelled. Checking the takes on disk…');
  await page.click('.batch-results-close');
  await expect(page.locator('#batch-results')).toBeHidden();
});
