// themed_confirm.js opened from a modal declared later in the DOM (#356),
// driven against tests/e2e/themed-confirm-nested.html with the real styles.
import { test, expect } from '@playwright/test';

test('a confirm opened from inside another modal sits on top and takes the click', async ({ page }) => {
  await page.goto('/tests/e2e/themed-confirm-nested.html');
  await page.waitForFunction(() => window.__harnessReady === true);

  await page.locator('#open-outer-btn').click();
  await page.locator('#outer-delete-btn').click();
  await expect(page.locator('#themed-confirm-modal')).toBeVisible();

  // The topmost element at the Confirm button's centre is the button itself,
  // not the outer modal's overlay.
  const box = await page.locator('#themed-confirm-ok-btn').boundingBox();
  const topmost = await page.evaluate(
    ([x, y]) => document.elementFromPoint(x, y)?.id,
    [box.x + box.width / 2, box.y + box.height / 2],
  );
  expect(topmost).toBe('themed-confirm-ok-btn');

  await page.locator('#themed-confirm-ok-btn').click({ timeout: 2000 });
  await expect(page.locator('#result')).toHaveText('true');
});
