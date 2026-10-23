// File → Open Recent (#935), driven against tests/e2e/recent-projects.html.
import { test, expect } from '@playwright/test';

async function gotoHarness(page, list) {
  await page.goto('/tests/e2e/recent-projects.html');
  await page.waitForFunction(() => window.__harnessReady === true);
  await page.evaluate((l) => window.__recent.load(l), list);
}

async function openRecentMenu(page) {
  await page.locator('#file-menu-btn').click();
  await page.locator('#recent-projects-menu').hover();
  await expect(page.locator('#recent-projects-panel')).toBeVisible();
}

const loads = (page) => page.evaluate(() =>
  window.__mockInvocations.filter((c) => c.cmd === 'load_project_session').map((c) => c.args.path));

test('the list shows each file name with its folder, and opens one like Load Session', async ({ page }) => {
  await gotoHarness(page, ['D:/projects/monday.json', 'D:/projects/old/wsod25.json']);
  await openRecentMenu(page);
  const items = page.locator('.recent-project-item');
  await expect(items).toHaveCount(2);
  await expect(items.nth(0).locator('.recent-project-name')).toHaveText('monday.json');
  await expect(items.nth(1).locator('.recent-project-folder')).toHaveText('D:/projects/old');
  await expect(items.nth(1)).toHaveAttribute('title', 'D:/projects/old/wsod25.json');

  await items.nth(1).click();
  await expect(page.locator('#session-file-indicator')).toHaveText('wsod25.json');
  expect(await loads(page)).toEqual(['D:/projects/old/wsod25.json']);
  // The menu closed, and the opened project went to the top and was saved.
  await expect(page.locator('#file-menu-panel')).toBeHidden();
  expect(await page.evaluate(() => window.__recent.list()))
    .toEqual(['D:/projects/old/wsod25.json', 'D:/projects/monday.json']);
  expect(await page.evaluate(() => window.__saved.at(-1)))
    .toEqual(['D:/projects/old/wsod25.json', 'D:/projects/monday.json']);
});

test('unsaved changes are asked about first, and Cancel opens nothing', async ({ page }) => {
  await gotoHarness(page, ['D:/projects/monday.json']);
  await page.evaluate(() => window.__markDirty());
  await openRecentMenu(page);
  await page.locator('.recent-project-item').click();
  await expect(page.locator('#unsaved-changes-modal')).toBeVisible();
  await page.locator('#unsaved-changes-cancel-btn').click();
  expect(await loads(page)).toEqual([]);

  await openRecentMenu(page);
  await page.locator('.recent-project-item').click();
  await page.locator('#unsaved-changes-discard-btn').click();
  await expect(page.locator('#session-file-indicator')).toHaveText('monday.json');
  expect(await loads(page)).toEqual(['D:/projects/monday.json']);
});

test('a file that is gone is dimmed, and choosing it offers to remove it', async ({ page }) => {
  await gotoHarness(page, ['D:/projects/monday.json', 'D:/projects/gone.json']);
  await page.evaluate(() => { window.__missing = ['D:/projects/gone.json']; });
  await openRecentMenu(page);
  const gone = page.locator('.recent-project-item', { hasText: 'gone.json' });
  await expect(gone).toHaveClass(/missing/);
  await expect(gone.locator('.recent-project-name')).toHaveText('gone.json (missing)');
  await expect(page.locator('.recent-project-item', { hasText: 'monday.json' })).not.toHaveClass(/missing/);

  await gone.click();
  await expect(page.locator('#themed-confirm-modal')).toBeVisible();
  await expect(page.locator('#themed-confirm-ok-btn')).toHaveText('Remove from list');
  await page.locator('#themed-confirm-ok-btn').click();
  expect(await loads(page)).toEqual([]);
  expect(await page.evaluate(() => window.__recent.list())).toEqual(['D:/projects/monday.json']);
  expect(await page.evaluate(() => window.__saved.at(-1))).toEqual(['D:/projects/monday.json']);
});

test('Clear Recent empties the list, and an empty list says so with Clear Recent greyed out', async ({ page }) => {
  await gotoHarness(page, ['D:/projects/monday.json']);
  await openRecentMenu(page);
  await page.locator('#clear-recent-projects-btn').click();
  expect(await page.evaluate(() => window.__recent.list())).toEqual([]);

  await openRecentMenu(page);
  await expect(page.locator('.recent-project-item')).toHaveCount(0);
  await expect(page.locator('#recent-projects-panel')).toContainText('No recent sessions');
  await expect(page.locator('#clear-recent-projects-btn')).toBeDisabled();
});

test('clicking the Open Recent row itself keeps the File menu open', async ({ page }) => {
  await gotoHarness(page, ['D:/projects/monday.json']);
  await page.locator('#file-menu-btn').click();
  await page.locator('#recent-projects-menu > span').first().click();
  await expect(page.locator('#file-menu-panel')).toBeVisible();
});

test('opening a project says so on the queue\'s line, then while it looks for missing demos (#958)', async ({ page }) => {
  await gotoHarness(page, ['D:/projects/monday.json']);
  await page.evaluate(() => { window.__hold = true; });
  await openRecentMenu(page);
  await page.locator('.recent-project-item').click();

  const line = page.locator('#scan-status');
  const bar = page.locator('#scan-progress');
  await expect(line).toHaveText(/^Opening the project… · 0:0\d$/);
  await expect(bar).toHaveJSProperty('hidden', false);
  await expect(bar).toHaveClass(/indeterminate/);

  await expect.poll(() => page.evaluate(() => typeof window.__release.load)).toBe('function');
  await page.evaluate(() => window.__release.load());
  await expect(line).toHaveText(/^Checking the project's demos are where they were… · 0:0\d$/);
  // The backend says how many it looks for once the search starts.
  await page.evaluate(() => window.__mockEmit('missing_demos_search', { missing: 3 }));
  await expect(line).toHaveText(/^Looking for 3 missing demos… · 0:0\d$/);
  await expect(bar).toHaveClass(/indeterminate/);

  await expect.poll(() => page.evaluate(() => typeof window.__release.search)).toBe('function');
  await page.evaluate(() => window.__release.search());
  // Not the line from before the open (a last scan's "Done in…").
  await expect(line).toHaveText('Opened the project: 0 demos');
  await expect(bar).toHaveJSProperty('hidden', true);
  await expect(page.locator('#session-file-indicator')).toHaveText('monday.json');
});
