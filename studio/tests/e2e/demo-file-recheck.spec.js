// A queued demo renamed, moved or deleted while Studio is open
// (demo_file_recheck.js): the row turns missing when the window gets its
// focus back, and back again when the file returns; Launch Preview names the
// missing file instead of trying it. Driven against
// tests/e2e/demo-file-recheck.html. See tests/e2e/README.md.
import { test, expect } from '@playwright/test';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/demo-file-recheck.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const row = (page, name) => page.locator('#master-demo-table-body tr', { hasText: name });
const menu = (page) => page.locator('.context-menu');
const item = (page, label) => menu(page).locator('.context-menu-item', { hasText: label });
const toast = (page) => page.locator('.toast');

// Explorer renames harrington.dem to harringtonx.dem while Studio is in the background.
const renameInExplorer = (page) => page.evaluate(() => {
  window.__disk.delete('D:/tests/harrington.dem');
  window.__disk.add('D:/tests/harringtonx.dem');
  window.__renamed['D:/tests/harrington.dem'] = 'D:/tests/harringtonx.dem';
});
// The user comes back to the window; the clock steps past the focus throttle.
const comeBack = (page) => page.evaluate(() => {
  window.__clock += 10000;
  window.dispatchEvent(new Event('focus'));
});
const invoked = (page, cmd) => page.evaluate((c) => window.__mockInvocations.filter((i) => i.cmd === c).length, cmd);

test.describe('a demo file that changes while Studio is open', () => {
  test('renamed in Explorer: on focus its row turns missing with Locate and Use found copy', async ({ page }) => {
    await gotoHarness(page);
    await expect(row(page, 'harrington.dem').locator('.locate-demo-btn')).toHaveCount(0);
    await renameInExplorer(page);
    await comeBack(page);
    const harrington = row(page, 'harrington.dem');
    await expect(harrington.locator('.locate-demo-btn')).toBeVisible();
    await expect(harrington.locator('.use-found-copy-btn')).toHaveAttribute('title', /harringtonx\.dem/);
    await expect(row(page, 'alpha.dem').locator('.locate-demo-btn')).toHaveCount(0);
    await expect(toast(page)).toContainText('No longer where the queue says: harrington.dem');
    const logged = await page.evaluate(() => window.__mockInvocations
      .filter((i) => i.cmd === 'log_frontend_event').map((i) => i.args.message));
    expect(logged.filter((l) => l.includes('looked at 2 demo file(s) again'))).toHaveLength(1);
  });

  test('once it is missing, the right-click menu dims what needs the file', async ({ page }) => {
    await gotoHarness(page);
    await renameInExplorer(page);
    await comeBack(page);
    await expect(row(page, 'harrington.dem').locator('.locate-demo-btn')).toBeVisible();
    await row(page, 'harrington.dem').click({ button: 'right' });
    await expect(item(page, 'Launch Preview')).toHaveClass(/dimmed/);
    await expect(item(page, 'Open in Demo Analyzer')).toHaveClass(/dimmed/);
    await expect(item(page, 'Use found copy')).toBeVisible();
    await expect(item(page, 'Locate demo')).toBeVisible();
  });

  test('a file put back clears the missing state on the next focus', async ({ page }) => {
    await gotoHarness(page);
    await renameInExplorer(page);
    await comeBack(page);
    await expect(row(page, 'harrington.dem').locator('.locate-demo-btn')).toBeVisible();
    await page.evaluate(() => window.__disk.add('D:/tests/harrington.dem'));
    await comeBack(page);
    await expect(row(page, 'harrington.dem').locator('.locate-demo-btn')).toHaveCount(0);
    await expect(row(page, 'harrington.dem').locator('.use-found-copy-btn')).toHaveCount(0);
  });

  test('Use found copy points the row at the renamed file', async ({ page }) => {
    await gotoHarness(page);
    await renameInExplorer(page);
    await comeBack(page);
    await row(page, 'harrington.dem').locator('.use-found-copy-btn').click();
    await expect(row(page, 'harringtonx.dem')).toBeVisible();
    await expect(row(page, 'harringtonx.dem').locator('.locate-demo-btn')).toHaveCount(0);
  });

  test('Launch Preview on a file renamed away says it is missing and to use Locate, and launches nothing', async ({ page }) => {
    await gotoHarness(page);
    await row(page, 'harrington.dem').click();
    await expect(page.locator('#detail-demo-title')).toContainText('harrington.dem');
    // No focus event: the window never lost it (a rename by another program).
    await renameInExplorer(page);
    await page.click('#btn-launch-preview');
    await expect(toast(page)).toContainText('Not found: harrington.dem. It was renamed, moved or deleted.');
    await expect(toast(page)).toContainText('Use Locate… on its row');
    await expect(toast(page)).not.toContainText('entities');
    await expect(row(page, 'harrington.dem').locator('.locate-demo-btn')).toBeVisible();
    await expect(page.locator('#btn-launch-preview')).toBeEnabled();
    expect(await invoked(page, 'launch_demo_preview')).toBe(0);
  });

  test('Open in Demo Analyzer from the menu on a file renamed away says so instead', async ({ page }) => {
    await gotoHarness(page);
    await renameInExplorer(page);
    await row(page, 'harrington.dem').click({ button: 'right' });
    await item(page, 'Open in Demo Analyzer').click();
    await expect(toast(page)).toContainText('Not found: harrington.dem.');
  });
});
