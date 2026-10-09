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

test.describe('master_pane multi-map demos (#217)', () => {
  test('a demo with two maps gets a Split button that names them on hover', async ({ page }) => {
    await gotoHarness(page);
    const split = row(page, 'here.dem').locator('.multimap-split-btn');
    await expect(split).toHaveText('2 maps · Split');
    await expect(split).toHaveAttribute('title', /recorded 2 maps \(dod_lennon2, dod_lennon2\)/);
  });

  test('clicking it hands the demo over without selecting the row', async ({ page }) => {
    await gotoHarness(page);
    await row(page, 'here.dem').locator('.multimap-split-btn').click();
    await expect(page.locator('#result')).toHaveText('split here.dem');
    await expect(row(page, 'here.dem')).not.toHaveClass(/table-row-selected/);
  });

  test('a one-map demo, or one saved before the map list existed, has none', async ({ page }) => {
    await gotoHarness(page);
    await expect(row(page, 'moved.dem').locator('.multimap-split-btn')).toHaveCount(0);
    await expect(row(page, 'gone.dem').locator('.multimap-split-btn')).toHaveCount(0);
  });
});

test.describe('master_pane search clear button (#529)', () => {
  const search = (page) => page.locator('#demo-search-input');
  const x = (page) => page.locator('.clearable-x');
  const rows = (page) => page.locator('#master-demo-table-body tr');

  test('the × shows only while the box has text', async ({ page }) => {
    await gotoHarness(page);
    await expect(x(page)).toBeHidden();
    await search(page).fill('moved');
    await expect(x(page)).toBeVisible();
    await search(page).fill('');
    await expect(x(page)).toBeHidden();
  });

  test('clicking it clears the text, re-filters and keeps focus in the box', async ({ page }) => {
    await gotoHarness(page);
    await search(page).fill('moved');
    await expect(rows(page)).toHaveCount(1);
    await x(page).click();
    await expect(search(page)).toHaveValue('');
    await expect(rows(page)).toHaveCount(3);
    await expect(search(page)).toBeFocused();
    await expect(x(page)).toBeHidden();
  });

  test('Esc in the box does the same', async ({ page }) => {
    await gotoHarness(page);
    await search(page).fill('gone');
    await expect(rows(page)).toHaveCount(1);
    await search(page).press('Escape');
    await expect(search(page)).toHaveValue('');
    await expect(rows(page)).toHaveCount(3);
  });
});
