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
