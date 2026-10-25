// The Demo Analyzer's Explorer tree (#572), driven against
// tests/e2e/analyzer-tree.html.
import { test, expect } from '@playwright/test';

// A Windows path inside a CSS attribute selector: each backslash doubled.
const css = (path) => path.replace(/\\/g, '\\\\');
const toggle = (page, path) => page.locator(`.tree-toggle[data-path="${css(path)}"]`);
const label = (page, path) => page.locator(`.tree-label[data-path="${css(path)}"]`);

async function gotoHarness(page) {
  await page.goto('/tests/e2e/analyzer-tree.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

test('an open folder still lists its subfolders after Refresh from elsewhere', async ({ page }) => {
  await gotoHarness(page);
  await toggle(page, 'D:\\').click();
  await toggle(page, 'D:\\Program Files (x86)').click();
  await expect(label(page, 'D:\\Program Files (x86)\\Steam')).toBeVisible();

  // Go somewhere else, then Refresh: Program Files (x86) stays open.
  await label(page, 'D:\\Users').click();
  await page.locator('#analyzer-tree-refresh-btn').click();

  await expect(label(page, 'D:\\Program Files (x86)\\Steam')).toBeVisible();
  await expect(page.locator('.tree-loading')).toHaveCount(0);
});

// A demo opened from Explorer deep in a cloud folder (#973): the list showed
// the folder and selected the demo only once every folder above it had been
// read, and every demo in all of them had been opened for its map.
const row = (page, path) => page.locator(`#analyzer-demo-tbody tr[data-path="${css(path)}"]`);

test('an opened demo is listed and selected while the folders above it are still read', async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate(() => { window.__held.add('D:\\Users'); window.__holdHeads = true; });
  // The analysis never finishes here, so its promise isn't handed back.
  await page.evaluate(() => { window.__openDemo('D:\\Users\\chris\\x.dem'); });

  const x = row(page, 'D:\\Users\\chris\\x.dem');
  await expect(x).toHaveClass(/selected/);
  await expect(row(page, 'D:\\Users\\chris\\a.dem')).not.toHaveClass(/selected/);
  // The map waits for the start of the file, and says so.
  await expect(x.locator('td').nth(2)).toHaveText('…');

  await page.evaluate(() => window.__releaseHeads());
  await expect(x.locator('td').nth(2)).toHaveText('dod_anzio');

  // The tree reaches the folder once the folder above it is read.
  await expect(label(page, 'D:\\Users\\chris')).toHaveCount(0);
  await page.evaluate(() => window.__release['D:\\Users']());
  await expect(label(page, 'D:\\Users\\chris').locator('xpath=..')).toHaveClass(/selected/);
});

test("the Demos list says it is reading a folder whose names aren't in yet", async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate(() => window.__held.add('D:\\Users\\chris'));
  await page.evaluate(() => { window.__openDemo('D:\\Users\\chris\\x.dem'); });
  await expect(page.locator('#analyzer-demo-tbody')).toHaveText('Reading this folder…');
  await page.evaluate(() => window.__release['D:\\Users\\chris']());
  await expect(row(page, 'D:\\Users\\chris\\x.dem')).toHaveClass(/selected/);
});

test('an open folder that cannot be read shows empty, not Loading… for good', async ({ page }) => {
  await gotoHarness(page);
  await toggle(page, 'D:\\').click();
  await toggle(page, 'D:\\Program Files (x86)').click();
  await expect(label(page, 'D:\\Program Files (x86)\\Steam')).toBeVisible();

  // It becomes unreadable, and everything is re-read from elsewhere.
  await page.evaluate(() => window.__unreadable.add('D:\\Program Files (x86)'));
  await label(page, 'D:\\Users').click();
  await page.locator('#analyzer-tree-refresh-btn').click();

  await expect(page.locator('.tree-loading')).toHaveCount(0);
  await expect(label(page, 'D:\\Program Files (x86)\\Steam')).toHaveCount(0);
});

// #992: a folder whose players are cached shows them from one lookup, and
// only the demos not read yet are read one by one; a cloud demo is not read.
test('a folder read before shows its players with no Reading players pass', async ({ page }) => {
  await gotoHarness(page);
  const reads = () => page.evaluate(() => window.__mockInvocations
    .filter((c) => c.cmd === 'index_demo_players' && c.args.paths.length > 0)
    .map((c) => c.args.paths));
  await page.evaluate((path) => {
    window.__cachedPlayers.add(path);
    // A read that is still going, so its counter stays up.
    window.__mockInvokeHandlers.index_demo_players = ({ paths }) => (paths.length ? new Promise(() => {}) : 0);
  }, 'D:\\Users\\chris\\a.dem');
  await toggle(page, 'D:\\').click();
  await toggle(page, 'D:\\Users').click();
  await label(page, 'D:\\Users\\chris').click();
  await expect.poll(reads).toEqual([['D:\\Users\\chris\\x.dem']]);
  await expect(page.locator('#analyzer-player-status')).toHaveText('Reading players: 0 / 1 not read yet');

  // x.dem is read now (its players file written); open the folder again.
  await page.evaluate((path) => window.__cachedPlayers.add(path), 'D:\\Users\\chris\\x.dem');
  await label(page, 'D:\\Users').click();
  await label(page, 'D:\\Users\\chris').click();
  await expect(page.locator('#analyzer-demo-tbody tr')).toHaveCount(2);
  await expect(page.locator('#analyzer-player-status')).toHaveText('');
  expect(await reads()).toHaveLength(1);
});

test("a cloud folder's demos with no cache entry are not read", async ({ page }) => {
  await gotoHarness(page);
  await page.evaluate((paths) => paths.forEach((p) => window.__cloud.add(p)),
    ['D:\\Users\\chris\\a.dem', 'D:\\Users\\chris\\x.dem']);
  await toggle(page, 'D:\\').click();
  await toggle(page, 'D:\\Users').click();
  await label(page, 'D:\\Users\\chris').click();
  await expect(page.locator('#analyzer-player-status')).toContainText('2 demos on a cloud drive');
  const reads = await page.evaluate(() => window.__mockInvocations
    .filter((c) => c.cmd === 'index_demo_players' && c.args.paths.length > 0).length);
  expect(reads).toBe(0);
});
