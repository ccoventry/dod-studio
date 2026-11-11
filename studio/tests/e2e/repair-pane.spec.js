// repair_pane.js — the Demo Auditor's Damaged Demos tab (#15): find the demos
// with a gap in the Target Folder, repair one or the ticked ones into
// <name>_repaired.dem, and show what each repair came to.
import { test, expect } from '@playwright/test';

const ONE = 'C:\\demos\\monday-wsod25_r07_m1_h1.dem';
const TWO = 'C:\\demos\\sub\\monday-wsod25_r07_m3_h2.dem';
const found = [
  { path: ONE, size_bytes: 82361144, first_hole: { after_secs: 39.2, byte_offset: 2433784, bytes_unread: 79927172 }, repaired: null },
  { path: TWO, size_bytes: 86458948, first_hole: { after_secs: 636.7, byte_offset: 42043000, bytes_unread: 44415948 }, repaired: 'C:\\demos\\sub\\monday-wsod25_r07_m3_h2_repaired.dem' },
];
const REPAIRED = {
  verdict: 'repaired',
  written: 'x',
  holes: [{ before_secs: 39.2, after_secs: 68.6, lost_secs: 29.4, bytes_skipped: 1899767 }],
  seconds_lost: 29.4,
  percent_kept: 96.7,
  ends_at_last_hole: false,
  reasons: [],
};

test.beforeEach(async ({ page }) => {
  await page.goto('/tests/e2e/repair-pane.html');
  await page.waitForFunction(() => window.__harnessReady === true);
  await page.evaluate((found) => {
    window.__mockInvokeHandlers.find_damaged_demos_cmd = () => new Promise((resolve) => { window.__finishFind = () => resolve(found); });
  }, found);
  await page.fill('#audit-target-folder-input', 'C:\\demos');
});

async function find(page) {
  await page.click('#repair-find-btn');
  await page.waitForFunction(() => typeof window.__finishFind === 'function');
  await page.evaluate(() => window.__finishFind());
  await expect(page.locator('#repair-status')).toHaveText('2 damaged demos.');
}

test('Find waits for a folder, shows its progress, then lists each damaged demo', async ({ page }) => {
  await page.fill('#audit-target-folder-input', '');
  await expect(page.locator('#repair-find-btn')).toBeDisabled();
  await page.fill('#audit-target-folder-input', 'C:\\demos');
  await page.click('#repair-find-btn');
  await expect(page.locator('#repair-find-btn')).toBeDisabled();
  await page.evaluate(() => window.__mockEmit('repair_scan_progress', { done: 3, total: 12, demo: 'a.dem' }));
  await expect(page.locator('#repair-status')).toHaveText('Checking demos: 3 of 12 (a.dem)');
  await expect(page.locator('#repair-progress .progress-bar-fill')).toHaveAttribute('style', /width: 25%/);
  await page.evaluate(() => window.__finishFind());
  await expect(page.locator('#repair-status')).toHaveText('2 damaged demos.');
  const rows = page.locator('#repair-body tr');
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0)).toContainText('monday-wsod25_r07_m1_h1.dem');
  await expect(rows.nth(0)).toContainText('0:39 (76.2 MB unread)');
  await expect(rows.nth(1).locator('.split-done-chip')).toHaveText('Repaired before');
  await expect(page.locator('#footer-audit-summary')).toHaveText('Damaged demos: 2 | Repaired: 0');
  await expect.poll(() => page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'find_damaged_demos_cmd')?.args))
    .toEqual({ folder: 'C:\\demos', recursive: true });
});

test('a row\'s Repair shows its step as it goes, then what was lost and where it was saved', async ({ page }) => {
  await page.evaluate(() => window.__holdRepair());
  await find(page);
  const row = page.locator('#repair-body tr').nth(0);
  await row.locator('.repair-row-btn').click();
  await expect(row.locator('.repair-row-btn')).toBeDisabled();
  await expect(row.locator('.split-progress-text')).toHaveText('Finding the gaps… 0%');
  await page.evaluate((path) => window.__mockEmit('repair_progress', { path, progress: { step: 'reading', fraction: 0.35 } }), ONE);
  await expect(row.locator('.split-progress-text')).toHaveText('Reading the recording… 35%');
  await page.evaluate((r) => window.__finishRepair(r), REPAIRED);
  await expect(row.locator('.repair-result')).toContainText('Repaired: 1 gap cut out (29 s of recording lost, 96.7% kept). Saved as monday-wsod25_r07_m1_h1_repaired.dem.');
  await expect(row.locator('.repair-result')).toHaveAttribute('title', 'Gaps, in time since the recording started: 0:39–1:08 (29 s)');
  await expect(row.locator('.split-progress')).toHaveCount(0);
  await expect(row.locator('.repair-show-btn')).toHaveText('Show in folder');
  await expect(page.locator('#footer-audit-summary')).toHaveText('Damaged demos: 2 | Repaired: 1');
});

test('Repair Checked repairs only the ticked demos, and says when a repair was not saved', async ({ page }) => {
  await page.evaluate(() => {
    window.__mockInvokeHandlers.repair_demo_cmd = () => ({
      verdict: 'refused', written: null, holes: [], seconds_lost: 0, percent_kept: 0, ends_at_last_hole: false,
      reasons: ['3 entity counts disagree with what the game holds'],
    });
  });
  await find(page);
  await page.locator('#repair-body tr').nth(1).locator('.repair-tick').uncheck();
  await page.click('#repair-checked-btn');
  await expect(page.locator('#repair-status')).toHaveText('Repaired 0 of 1 demo.');
  const calls = await page.evaluate(() => window.__mockInvocations.filter((c) => c.cmd === 'repair_demo_cmd').map((c) => c.args.path));
  expect(calls).toEqual([ONE]);
  await expect(page.locator('#repair-body tr').nth(0).locator('.repair-result'))
    .toHaveText('Not saved: the repaired demo failed a check (3 entity counts disagree with what the game holds).');
});
