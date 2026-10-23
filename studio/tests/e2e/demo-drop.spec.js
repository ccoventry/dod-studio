// demo_drop.js (#530), driven against tests/e2e/demo-drop.html.
import { test, expect } from '@playwright/test';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/demo-drop.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

// Physical pixels, as Tauri sends them; the harness page has a scale of 1.
const over = (x, y) => ({ type: 'over', position: { x, y } });

test('both zones light up while dragging, the one under the pointer more', async ({ page }) => {
  await gotoHarness(page);
  const scan = page.locator('#scan-panel');
  const queue = page.locator('#master-pane');
  await page.evaluate(() => window.__drag({ type: 'enter', paths: ['D:/a.dem'], position: { x: 600, y: 50 } }));
  await expect(scan).toHaveClass(/drop-zone-active/);
  await expect(queue).toHaveClass(/drop-zone-active/);
  await expect(scan).not.toHaveClass(/drop-zone-hover/);

  await page.evaluate((p) => window.__drag(p), over(50, 200));
  await expect(queue).toHaveClass(/drop-zone-hover/);
  await expect(scan).not.toHaveClass(/drop-zone-hover/);

  await page.evaluate(() => window.__drag({ type: 'leave' }));
  await expect(scan).not.toHaveClass(/drop-zone-active/);
  await expect(queue).not.toHaveClass(/drop-zone-active/);
});

test('a mixed drop on the queue scans the demos and folders together and names what it skipped', async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate(() => window.__drag({
    type: 'drop',
    paths: ['D:/match.DEM', 'D:/demos', 'D:/notes.txt', 'D:/project.json'],
    position: { x: 50, y: 200 },
  }));
  await expect.poll(() => page.evaluate(() => window.__added)).toEqual([
    { files: ['D:/match.DEM'], folders: ['D:/demos'] },
  ]);
  await expect(page.locator('.toast', { hasText: 'Not a demo or a folder, so not added: notes.txt, project.json' })).toBeVisible();
  await expect(page.locator('#master-pane')).not.toHaveClass(/drop-zone-active/);
});

test('a drop on Directory Scan works too, and one elsewhere is ignored', async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate(() => window.__drag({ type: 'drop', paths: ['D:/x.dem'], position: { x: 600, y: 50 } }));
  await page.evaluate(() => window.__drag({ type: 'drop', paths: ['D:/y.dem'], position: { x: 10, y: 10 } }));
  await expect.poll(() => page.evaluate(() => window.__added)).toEqual([{ files: ['D:/y.dem'], folders: [] }]);
  const sorts = await page.evaluate(() => window.__mockInvocations.filter((c) => c.cmd === 'sort_dropped_paths'));
  expect(sorts).toHaveLength(1);
});

test('a drop while a scan runs is refused with a toast', async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate(() => { window.__busy = true; });
  await page.evaluate(() => window.__drag({ type: 'drop', paths: ['D:/x.dem'], position: { x: 10, y: 10 } }));
  await expect(page.locator('.toast', { hasText: 'A scan is running' })).toBeVisible();
  expect(await page.evaluate(() => window.__added)).toEqual([]);
});
