// queue_scan.js's folder listing (#843), driven against tests/e2e/queue-scan.html.
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
  await expect(status).toHaveText('Status: Scanning...');
  await expect(cancel).toBeEnabled();
  await expect(page.locator('#add-folder-btn')).toBeDisabled();
  await expect(page.locator('#scan-progress')).not.toHaveAttribute('hidden');

  await page.evaluate(() => window.__mockEmit('scan_progress', {
    scanned: 0, found: 0, cancelled: false, listing: true, folders: 120, demos: 3,
    status: 'Looking for demos: 120 folders read, 3 demos found',
  }));
  await expect(status).toHaveText('Status: Looking for demos: 120 folders read, 3 demos found');
  await expect(cancel).toBeEnabled();

  await cancel.click();
  const cancels = await page.evaluate(() => window.__mockInvocations.filter((c) => c.cmd === 'cancel_scan'));
  expect(cancels).toHaveLength(1);
  // The status line says it is stopping, rather than a toast that the
  // cancelled one would follow a moment later.
  await expect(status).toHaveText('Status: Cancelling…');
  await expect(cancel).toBeDisabled();
  // A listing line that was already on its way doesn't take that back.
  await page.evaluate(() => window.__mockEmit('scan_progress', {
    scanned: 0, found: 0, cancelled: false, listing: true, folders: 130, demos: 3,
    status: 'Looking for demos: 130 folders read, 3 demos found',
  }));
  await expect(status).toHaveText('Status: Cancelling…');

  await page.evaluate(() => window.__mockEmit('scan_progress', {
    scanned: 0, found: 0, status: 'Cancelled', cancelled: true, unchanged: 0, skipped: [],
  }));
  await expect(status).toHaveText('Status: Cancelled — 0 demo(s) found before cancel');

  await page.evaluate(() => window.__finishScan({ demos: [], unchanged: 0, copies: [], cancelled: true }));
  await page.evaluate(() => window.__added);
  await expect(page.locator('.toast', { hasText: 'Scan cancelled, nothing added' })).toBeVisible();
  await expect(page.locator('.toast', { hasText: 'Scan complete' })).toHaveCount(0);
  await expect(page.locator('.toast', { hasText: 'cancellation requested' })).toHaveCount(0);
  // A drive the user gave up on is not remembered.
  expect(await page.evaluate(() => window.__recent)).toEqual([]);
  await expect(page.locator('#add-folder-btn')).toBeEnabled();
  await expect(cancel).toBeDisabled();
  await expect(page.locator('#scan-progress')).toHaveAttribute('hidden');
});

test('a folder whose scan finishes goes into Recent (#533)', async ({ page }) => {
  await gotoHarness(page);
  await startAddFolder(page, 'D:/demos');
  await page.evaluate(() => window.__finishScan({ demos: [], unchanged: 0, copies: [], cancelled: false }));
  await page.evaluate(() => window.__added);
  await expect(page.locator('.toast', { hasText: 'Scan complete (0 demo(s) found)' })).toBeVisible();
  expect(await page.evaluate(() => window.__recent)).toEqual(['D:/demos']);
});

test('Include subfolders goes with the scan and is saved when changed (#531)', async ({ page }) => {
  await gotoHarness(page);
  await startAddFolder(page, 'D:/demos');
  await page.evaluate(() => window.__finishScan({ demos: [], unchanged: 0, copies: [], cancelled: false }));
  await page.evaluate(() => window.__added);
  let scans = await page.evaluate(() => window.__mockInvocations.filter((c) => c.cmd === 'scan_directory'));
  expect(scans[0].args.includeSubfolders).toBe(true);

  await page.locator('#scan-include-subfolders').uncheck();
  expect(await page.evaluate(() => window.__saves)).toBe(1);
  await page.evaluate(() => { window.__finishScan = null; });
  await startAddFolder(page, 'D:/demos');
  await page.evaluate(() => window.__finishScan({ demos: [], unchanged: 0, copies: [], cancelled: false }));
  await page.evaluate(() => window.__added);
  scans = await page.evaluate(() => window.__mockInvocations.filter((c) => c.cmd === 'scan_directory'));
  expect(scans[1].args.includeSubfolders).toBe(false);
});

test('a scan stopped by the depth cap says so in a toast (#531)', async ({ page }) => {
  await gotoHarness(page);
  await startAddFolder(page, 'D:/');
  await page.evaluate(() => window.__finishScan({
    demos: [], unchanged: 0, copies: [], cancelled: false,
    cap: { depth: true, entries: false, max_depth: 6, max_entries: 50000 },
  }));
  await page.evaluate(() => window.__added);
  await expect(page.locator('.toast', { hasText: 'folders more than 6 levels down were not read' })).toBeVisible();
});

test('dropped demos and folders scan together, and the folders go into Recent (#530)', async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate(() => { window.__added = window.__addDropped(['D:/a.dem'], ['D:/one', 'D:/two']); });
  await page.waitForFunction(() => typeof window.__finishScan === 'function');
  const scans = await page.evaluate(() => window.__mockInvocations.filter((c) => c.cmd === 'scan_directory'));
  expect(scans[0].args.paths).toEqual(['D:/a.dem', 'D:/one', 'D:/two']);
  await page.evaluate(() => window.__finishScan({ demos: [], unchanged: 0, copies: [], cancelled: false }));
  await page.evaluate(() => window.__added);
  // The first folder dropped ends up on top.
  expect(await page.evaluate(() => window.__recent)).toEqual(['D:/one', 'D:/two']);
});
