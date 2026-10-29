// "After a clip is finished" (#644), driven against
// tests/e2e/take-cleanup.html — see that file and tests/e2e/README.md.
import { test, expect } from '@playwright/test';

async function gotoHarness(page, query = '') {
  await page.goto(`/tests/e2e/take-cleanup.html${query}`);
  await page.waitForFunction(() => window.__cleanupReady === true);
}

const setting = (page) => page.evaluate(() => window.__getSetting());
const saves = (page) => page.evaluate(() => window.__testSettingsChangeCount);
const emit = (page, name, payload) => page.evaluate(([n, p]) => window.__mockEmit(n, p), [name, payload]);

test('keeps takes by default, with "Only when space is needed" off', async ({ page }) => {
  await gotoHarness(page);
  await expect(page.locator('#config-take-after-finish')).toHaveValue('keep');
  await expect(page.locator('#config-take-delete-when-needed')).toBeDisabled();
  await expect(page.locator('#config-take-after-finish-hint')).toContainText('Takes stay on disk');
  expect(await setting(page)).toEqual({ take_after_finish: 'keep', take_delete_only_when_space_needed: false });
});

test('the Recycle Bin needs no confirm, and says it frees no space until emptied', async ({ page }) => {
  await gotoHarness(page);
  await page.selectOption('#config-take-after-finish', 'recycle');
  await expect(page.locator('#themed-confirm-modal')).toBeHidden();
  await expect(page.locator('#config-take-after-finish-hint')).toContainText('frees no space until you empty it');
  await expect(page.locator('#config-take-delete-when-needed')).toBeDisabled();
  await expect(page.locator('#config-take-delete-when-needed')).toHaveAttribute('title', /Recycle Bin frees no space/);
  expect(await saves(page)).toBe(1);
});

test('turning permanent deletion on asks first, and Cancel puts the old choice back', async ({ page }) => {
  await gotoHarness(page);
  await page.selectOption('#config-take-after-finish', 'delete');
  await expect(page.locator('#themed-confirm-modal')).toBeVisible();
  await expect(page.locator('#themed-confirm-title')).toHaveText('Delete takes permanently?');
  await page.click('#themed-confirm-cancel-btn');
  await expect(page.locator('#config-take-after-finish')).toHaveValue('keep');
  await expect(page.locator('#config-take-delete-when-needed')).toBeDisabled();
  expect(await saves(page)).toBe(0);
});

test('a confirmed permanent choice unlocks "Only when space is needed"', async ({ page }) => {
  await gotoHarness(page);
  await page.selectOption('#config-take-after-finish', 'delete');
  await expect(page.locator('#themed-confirm-ok-btn')).toHaveText('Delete permanently');
  await page.click('#themed-confirm-ok-btn');
  await expect(page.locator('#config-take-after-finish')).toHaveValue('delete');
  await expect(page.locator('#config-take-after-finish-hint')).toContainText('cannot be restored');
  const box = page.locator('#config-take-delete-when-needed');
  await expect(box).toBeEnabled();
  await box.check();
  await expect(page.locator('#config-take-after-finish-hint')).toContainText('short of room');
  expect(await setting(page)).toEqual({ take_after_finish: 'delete', take_delete_only_when_space_needed: true });
  expect(await saves(page)).toBe(2);
});

test('a saved permanent choice loads without asking', async ({ page }) => {
  await gotoHarness(page, '?saved=delete');
  await expect(page.locator('#config-take-after-finish')).toHaveValue('delete');
  await expect(page.locator('#config-take-delete-when-needed')).toBeChecked();
  await expect(page.locator('#themed-confirm-modal')).toBeHidden();
  expect(await saves(page)).toBe(0);
});

test('shows the backend\'s progress in the Render tab bar, then one toast', async ({ page }) => {
  await gotoHarness(page);
  await emit(page, 'take_cleanup_progress', { step: 'deleting', done: 1, total: 4, reason: 'capture' });
  await expect(page.locator('#render-take-delete-progress')).toBeVisible();
  await expect(page.locator('#render-take-delete-progress-label')).toHaveText('Deleting finished takes (1 of 4)');
  await expect(page.locator('#render-take-delete-progress .progress-bar-fill')).toHaveAttribute('style', /width:\s*25%/);

  await emit(page, 'take_cleanup_done', {
    reason: 'capture',
    method: 'delete',
    deleted: [{ take_folder: 'D:\\cap\\s1\\b0\\take0000', take_key: 's1/b0', size_bytes: 3 * 1073741824 }],
    failed: [],
    short: null,
  });
  await expect(page.locator('#render-take-delete-progress')).toBeHidden();
  await expect(page.locator('#toast-container')).toContainText('Deleted a finished take (3.00 GB) to make room for the capture.');
});
