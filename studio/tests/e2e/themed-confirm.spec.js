// themed_confirm.js — driven against tests/e2e/themed-confirm.html.
// See tests/e2e/README.md for what this style of test covers.
import { test, expect } from '@playwright/test';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/themed-confirm.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

test.describe('themed_confirm', () => {
  test('is hidden until triggered, and shows the message', async ({ page }) => {
    await gotoHarness(page);
    await expect(page.locator('#themed-confirm-modal')).toBeHidden();

    await page.locator('#trigger-default-btn').click();

    await expect(page.locator('#themed-confirm-modal')).toBeVisible();
    await expect(page.locator('#themed-confirm-message')).toHaveText('Delete 3 files?');
  });

  test('clicking Confirm resolves true and hides the modal', async ({ page }) => {
    await gotoHarness(page);
    await page.locator('#trigger-default-btn').click();
    await page.locator('#themed-confirm-ok-btn').click();

    await expect(page.locator('#themed-confirm-modal')).toBeHidden();
    await expect(page.locator('#result')).toHaveText('true');
  });

  test('clicking Cancel resolves false and hides the modal', async ({ page }) => {
    await gotoHarness(page);
    await page.locator('#trigger-default-btn').click();
    await page.locator('#themed-confirm-cancel-btn').click();

    await expect(page.locator('#themed-confirm-modal')).toBeHidden();
    await expect(page.locator('#result')).toHaveText('false');
  });

  test('custom title/labels override the defaults, and default labels come back on the next call', async ({ page }) => {
    await gotoHarness(page);
    await page.locator('#trigger-custom-btn').click();

    await expect(page.locator('#themed-confirm-title')).toHaveText('Remove Demo');
    await expect(page.locator('#themed-confirm-ok-btn')).toHaveText('Remove');
    await expect(page.locator('#themed-confirm-cancel-btn')).toHaveText('Keep It');
    await page.locator('#themed-confirm-ok-btn').click();

    await page.locator('#trigger-default-btn').click();
    await expect(page.locator('#themed-confirm-title')).toHaveText('Confirm');
    await expect(page.locator('#themed-confirm-ok-btn')).toHaveText('Confirm');
    await expect(page.locator('#themed-confirm-cancel-btn')).toHaveText('Cancel');
  });

  test('a second call while one is pending resolves the first call, not two independent answers', async ({ page }) => {
    await gotoHarness(page);
    // Fire both triggers back to back without waiting — the module only
    // tracks one pendingResolve, so the second call's modal state wins and
    // the one Promise in flight resolves to whatever button is actually
    // clicked. This documents that behavior rather than asserting a queue
    // that doesn't exist.
    await page.locator('#trigger-default-btn').click();
    await page.locator('#trigger-custom-btn').click();
    await expect(page.locator('#themed-confirm-message')).toHaveText('Remove this tracked demo?');

    await page.locator('#themed-confirm-ok-btn').click();
    await expect(page.locator('#result')).toHaveText('true');
  });
});
