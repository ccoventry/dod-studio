// queue_scan.js's folder listing (#843) and its progress line and bar (#951),
// driven against tests/e2e/queue-scan.html.
import { test, expect } from '@playwright/test';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/queue-scan.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

async function startAddFolder(page, folder) {
  await page.evaluate((f) => { window.__added = window.__addDemoFolder(f); }, folder);
  await page.waitForFunction(() => typeof window.__finishScan === 'function');
}

test('listing a drive shows its progress, and Cancel while listing adds nothing', async ({ page }) => {
  await gotoHarness(page);
  const status = page.locator('#scan-status');
  const cancel = page.locator('#cancel-scan-btn');

  await startAddFolder(page, 'F:/');
  // The moment the scan starts, before the backend has said anything.
  // It says so, with the clock, and the bar slides rather than sitting
  // empty (#951).
  await expect(status).toHaveText(/^Looking for demos… · 0:0\d$/);
  await expect(cancel).toBeEnabled();
  await expect(page.locator('#add-folder-btn')).toBeDisabled();
  await expect(page.locator('#scan-progress')).not.toHaveAttribute('hidden');
  await expect(page.locator('#scan-progress')).toHaveClass(/indeterminate/);
  await expect(page.locator('#scan-progress')).toBeVisible();

  await page.evaluate(() => window.__mockEmit('scan_progress', {
    scanned: 0, found: 0, cancelled: false, listing: true, folders: 120, demos: 3,
    status: 'Looking for demos: 120 folders read, 3 demos found',
  }));
  await expect(status).toHaveText(/^Looking for demos: 120 folders, 3 demos found · 0:0\d$/);
  // No earlier scan of F:/ to measure against: still sliding.
  await expect(page.locator('#scan-progress')).toHaveClass(/indeterminate/);
  await expect(cancel).toBeEnabled();

  await cancel.click();
  const cancels = await page.evaluate(() => window.__mockInvocations.filter((c) => c.cmd === 'cancel_scan'));
  expect(cancels).toHaveLength(1);
  // The status line says it is stopping, rather than a toast that the
  // cancelled one would follow a moment later.
  await expect(status).toHaveText(/^Cancelling… · 0:0\d$/);
  await expect(cancel).toBeDisabled();
  // A listing line that was already on its way doesn't take that back.
  await page.evaluate(() => window.__mockEmit('scan_progress', {
    scanned: 0, found: 0, cancelled: false, listing: true, folders: 130, demos: 3,
    status: 'Looking for demos: 130 folders read, 3 demos found',
  }));
  await expect(status).toHaveText(/^Cancelling… · 0:0\d$/);

  await page.evaluate(() => window.__mockEmit('scan_progress', {
    scanned: 0, found: 0, status: 'Cancelled', cancelled: true, unchanged: 0, skipped: [],
  }));
  await page.evaluate(() => window.__finishScan({ demos: [], unchanged: 0, copies: [], cancelled: true }));
  await page.evaluate(() => window.__added);
  await expect(status).toHaveText(/^Cancelled after \d+\.\d s: nothing added$/);
  await expect(page.locator('.toast', { hasText: 'Scan cancelled, nothing added' })).toBeVisible();
  await expect(page.locator('.toast', { hasText: 'Scan complete' })).toHaveCount(0);
  await expect(page.locator('.toast', { hasText: 'cancellation requested' })).toHaveCount(0);
  // A drive the user gave up on is not remembered.
  expect(await page.evaluate(() => window.__scanPaths)).toEqual([]);
  await expect(page.locator('#add-folder-btn')).toBeEnabled();
  await expect(cancel).toBeDisabled();
  await expect(page.locator('#scan-progress')).toHaveAttribute('hidden');
});

test('a folder whose scan finishes is remembered', async ({ page }) => {
  await gotoHarness(page);
  await startAddFolder(page, 'D:/demos');
  await page.evaluate(() => window.__finishScan({ demos: [], unchanged: 0, copies: [], cancelled: false }));
  await page.evaluate(() => window.__added);
  await expect(page.locator('.toast', { hasText: 'Scan complete (0 demo(s) found)' })).toBeVisible();
  expect(await page.evaluate(() => window.__scanPaths)).toEqual(['D:/demos']);
});

test('one bar from looking for demos, through reading them, to grouping them', async ({ page }) => {
  await gotoHarness(page);
  const status = page.locator('#scan-status');
  const bar = page.locator('#scan-progress');
  const width = () => page.evaluate(() => parseFloat(document.querySelector('#scan-progress .progress-bar-fill').style.width));

  // A first scan of D:/demos lists 40 folders, which the next one measures against.
  await startAddFolder(page, 'D:/demos');
  await page.evaluate(() => window.__mockEmit('scan_progress', {
    scanned: 0, found: 0, cancelled: false, listing: true, folders: 40, demos: 2,
  }));
  await page.evaluate(() => window.__finishScan({ demos: [], unchanged: 2, copies: [], cancelled: false }));
  await page.evaluate(() => window.__added);
  await expect(status).toHaveText(/^Done in \d+\.\d s: 2 already in the queue$/);
  await expect(bar).toHaveAttribute('hidden');

  await page.evaluate(() => { delete window.__finishScan; });
  await startAddFolder(page, 'D:/demos');
  await page.evaluate(() => window.__mockEmit('scan_progress', {
    scanned: 0, found: 0, cancelled: false, listing: true, folders: 20, demos: 1,
  }));
  await expect(status).toHaveText(/^Looking for demos: 20 folders \(about 40 last time\), 1 demo found · /);
  await expect(bar).not.toHaveClass(/indeterminate/);
  // Half of the last scan's folders, on the looking step's tenth of the bar.
  expect(await width()).toBe(5);

  await page.evaluate(() => window.__mockEmit('scan_progress', {
    scanned: 1, found: 1, cancelled: false, total: 2, cached: 0, parsed: 1,
    status: 'Reading demos: 1 of 2, b.dem',
  }));
  await expect(status).toHaveText(/^Reading demos: 1 of 2, b\.dem · /);
  expect(await width()).toBe(50);

  await page.evaluate(() => window.__mockEmit('scan_progress', {
    scanned: 2, found: 2, status: 'Complete', cancelled: false, unchanged: 0, skipped: [],
  }));
  await page.evaluate(() => window.__finishScan({
    demos: [{ path: 'D:/demos/a.dem', name: 'a.dem', streaks: [] }, { path: 'D:/demos/b.dem', name: 'b.dem', streaks: [] }],
    unchanged: 0, copies: [], cancelled: false,
  }));
  // The new demos are grouped by match on the same bar, still running.
  await page.waitForFunction(() => typeof window.__finishGrouping === 'function');
  await expect(status).toHaveText(/^Grouping by match… · /);
  expect(await width()).toBe(90);
  await expect(page.locator('#add-folder-btn')).toBeDisabled();
  await page.evaluate(() => window.__mockEmit('match_group_progress', { done: 1, total: 2 }));
  await expect(status).toHaveText(/^Grouping by match: 1 of 2 · /);
  expect(await width()).toBe(95);

  await page.evaluate(() => window.__finishGrouping());
  await page.evaluate(() => window.__added);
  await expect(status).toHaveText(/^Done in \d+\.\d s: 2 demos added$/);
  await expect(bar).toHaveAttribute('hidden');
  await expect(page.locator('#add-folder-btn')).toBeEnabled();
  await expect(page.locator('.toast', { hasText: 'Scanning' })).toHaveCount(0);
});
