// master_pane.js's missing-demo row (#21) — driven against
// tests/e2e/master-pane.html. See tests/e2e/README.md.
import { test, expect } from '@playwright/test';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/master-pane.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const row = (page, name) => page.locator('#master-demo-table-body tr', { hasText: name });

test.describe('master_pane missing demos', () => {
  test('a demo left as missing with a found copy offers it, with the path on hover', async ({ page }) => {
    await gotoHarness(page);
    const useBtn = row(page, 'moved.dem').locator('.use-found-copy-btn');
    await expect(useBtn).toHaveText('Use found copy');
    await expect(useBtn).toHaveAttribute('title', /C:\\demos\\sub\\moved\.dem/);
    await expect(row(page, 'moved.dem').locator('.locate-demo-btn')).toBeVisible();
  });

  test('clicking Use found copy hands that demo over without selecting the row', async ({ page }) => {
    await gotoHarness(page);
    await row(page, 'moved.dem').locator('.use-found-copy-btn').click();
    await expect(page.locator('#result')).toHaveText('use moved.dem');
  });

  test('a missing demo with nothing found, or a present one, has no Use found copy', async ({ page }) => {
    await gotoHarness(page);
    await expect(row(page, 'gone.dem').locator('.use-found-copy-btn')).toHaveCount(0);
    await expect(row(page, 'gone.dem').locator('.locate-demo-btn')).toBeVisible();
    await expect(row(page, 'here.dem').locator('.use-found-copy-btn')).toHaveCount(0);
    await expect(row(page, 'here.dem').locator('.locate-demo-btn')).toHaveCount(0);
  });
});
