// The Master Demo Queue row's right-click menu (#1111) — driven against
// tests/e2e/master-pane-menu.html. See tests/e2e/README.md.
import { test, expect } from '@playwright/test';

test.use({ permissions: ['clipboard-read', 'clipboard-write'] });

async function gotoHarness(page) {
  await page.goto('/tests/e2e/master-pane-menu.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const row = (page, name) => page.locator('#master-demo-table-body tr', { hasText: name });
const menu = (page) => page.locator('.context-menu');
const item = (page, label) => menu(page).locator('.context-menu-item', { hasText: label });

test.describe('queue row context menu', () => {
  test('right-click shows the menu for that row, and Copy Demo Name copies the name', async ({ page }) => {
    await gotoHarness(page);
    await row(page, 'bravo.dem').click({ button: 'right' });
    await expect(menu(page)).toBeVisible();
    await expect(row(page, 'bravo.dem')).toHaveClass(/context-target/);
    await item(page, 'Copy Demo Name').click();
    await expect(menu(page)).toHaveCount(0);
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('bravo.dem');
  });

  test('Copy Path copies the full path', async ({ page }) => {
    await gotoHarness(page);
    await row(page, 'alpha.dem').click({ button: 'right' });
    await item(page, 'Copy Path').click();
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('C:\demos\alpha.dem');
  });

  test('Esc closes it and gives the focus back to the row', async ({ page }) => {
    await gotoHarness(page);
    await row(page, 'alpha.dem').focus();
    await row(page, 'alpha.dem').click({ button: 'right' });
    await expect(menu(page)).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(menu(page)).toHaveCount(0);
    await expect(row(page, 'alpha.dem')).not.toHaveClass(/context-target/);
  });

  test('a click elsewhere closes it', async ({ page }) => {
    await gotoHarness(page);
    await row(page, 'alpha.dem').click({ button: 'right' });
    await page.mouse.click(5, 5);
    await expect(menu(page)).toHaveCount(0);
  });

  test('Shift+F10 on a focused row opens it on the first item; arrows and Enter choose', async ({ page }) => {
    await gotoHarness(page);
    await row(page, 'alpha.dem').focus();
    await page.keyboard.press('Shift+F10');
    await expect(menu(page)).toBeVisible();
    await expect(menu(page).locator('.context-menu-item.active')).toHaveText('Launch Preview');
    await page.keyboard.press('ArrowDown');
    await expect(menu(page).locator('.context-menu-item.active')).toHaveText('Review this demo');
    await page.keyboard.press('Enter');
    await expect(menu(page)).toHaveCount(0);
    await expect(page.locator('#result')).toHaveText('review C:\demos\alpha.dem');
  });

  test('the Menu key opens it too, and Up from the top wraps to Remove from Queue', async ({ page }) => {
    await gotoHarness(page);
    await row(page, 'bravo.dem').focus();
    await page.keyboard.press('ContextMenu');
    await expect(menu(page)).toBeVisible();
    await page.keyboard.press('ArrowUp');
    await expect(menu(page).locator('.context-menu-item.active')).toHaveText('Remove from Queue');
    await page.keyboard.press('Enter');
    await expect(page.locator('#result')).toHaveText('removed, 2 left');
  });

  test('Launch Preview selects the clicked row first, then presses the existing button', async ({ page }) => {
    await gotoHarness(page);
    await row(page, 'bravo.dem').click({ button: 'right' });
    await item(page, 'Launch Preview').click();
    await expect(page.locator('#result')).toHaveText('preview bravo.dem');
  });

  test('Split Maps is dimmed with the pieces named on a demo already split', async ({ page }) => {
    await page.addInitScript(() => {
      window.__mockInvokeHandlers = {
        demo_split_status: () => ({ split: true, files: ['two_split1_harrington.dem', 'two_split2_anzio.dem'] }),
      };
    });
    await gotoHarness(page);
    await page.evaluate(() => window.__renderTwoMaps());
    await expect(row(page, 'two.dem').locator('.multimap-split-note')).toBeVisible();
    await row(page, 'two.dem').click({ button: 'right' });
    await expect(item(page, 'Split Maps')).toHaveAttribute('aria-disabled', 'true');
    await expect(item(page, 'Split Maps')).toHaveAttribute('title', /two_split1_harrington\.dem/);
  });

  const DAMAGE = { damage: { read_up_to_secs: 600, frame_index: 1, byte_offset: 1, bytes_unread: 1, reason: 'x' } };

  test('Split Maps is dimmed with "Repair it first" on a damaged demo the repair can fix', async ({ page }) => {
    await page.addInitScript(() => {
      window.__mockInvokeHandlers = { demo_split_status: () => ({ split: false, files: [] }), demo_repairable: () => true };
    });
    await gotoHarness(page);
    await page.evaluate((extra) => window.__renderTwoMaps(extra), DAMAGE);
    await expect(row(page, 'two.dem').locator('.multimap-split-btn')).toBeDisabled();
    await row(page, 'two.dem').click({ button: 'right' });
    await expect(item(page, 'Split Maps')).toHaveAttribute('aria-disabled', 'true');
    await expect(item(page, 'Split Maps')).toHaveAttribute('title', /Repair it first/);
  });

  test('Split Maps runs on a damaged demo the repair finds no break in', async ({ page }) => {
    await page.addInitScript(() => {
      window.__mockInvokeHandlers = { demo_split_status: () => ({ split: false, files: [] }), demo_repairable: () => false };
    });
    await gotoHarness(page);
    await page.evaluate((extra) => window.__renderTwoMaps(extra), DAMAGE);
    await expect(row(page, 'two.dem').locator('.multimap-split-btn')).toBeEnabled();
    await row(page, 'two.dem').click({ button: 'right' });
    await item(page, 'Split Maps').click();
    await expect(page.locator('#result')).toHaveText('split two.dem');
  });

  test('Split Maps runs on a demo not split yet, and is dimmed while that split runs', async ({ page }) => {
    await page.addInitScript(() => {
      window.__mockInvokeHandlers = { demo_split_status: () => ({ split: false, files: [] }) };
    });
    await gotoHarness(page);
    await page.evaluate(() => window.__renderTwoMaps());
    await expect(row(page, 'two.dem').locator('.multimap-split-btn')).toBeVisible();
    await row(page, 'two.dem').click({ button: 'right' });
    await item(page, 'Split Maps').click();
    await expect(page.locator('#result')).toHaveText('split two.dem');
    await row(page, 'two.dem').click({ button: 'right' });
    await expect(item(page, 'Split Maps')).toHaveAttribute('aria-disabled', 'true');
    await expect(item(page, 'Split Maps')).toHaveAttribute('title', 'This demo is being split.');
  });

  test('a missing demo dims what needs the file, with the reason, and offers Locate', async ({ page }) => {
    await gotoHarness(page);
    await row(page, 'lost.dem').click({ button: 'right' });
    const preview = item(page, 'Launch Preview');
    await expect(preview).toHaveClass(/dimmed/);
    await expect(preview).toHaveAttribute('title', /missing/);
    await preview.click({ force: true });
    await expect(menu(page)).toBeVisible();
    await item(page, 'Locate demo').click();
    await expect(page.locator('#result')).toHaveText('locate lost.dem');
  });

  test('it stays inside a small window, opened near its bottom-right corner', async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 300 });
    await gotoHarness(page);
    const rowBox = await row(page, 'bravo.dem').boundingBox();
    await page.mouse.click(300, rowBox.y + 5, { button: 'right' });
    await expect(menu(page)).toBeVisible();
    const box = await menu(page).boundingBox();
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.y).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(320);
    expect(box.y + box.height).toBeLessThanOrEqual(300);
  });
  test('Launch Preview after the queue was rebuilt selects the demo at its new index', async ({ page }) => {
    await gotoHarness(page);
    await row(page, 'bravo.dem').click({ button: 'right' });
    await page.evaluate(() => window.__rerender(['bravo.dem', 'alpha.dem']));
    await item(page, 'Launch Preview').click();
    expect(await page.evaluate(() => window.__selected)).toBe('bravo.dem@0');
    await expect(page.locator('#result')).toHaveText('preview bravo.dem@0');
  });

  test('Launch Preview does nothing when the demo left the queue while the menu was open', async ({ page }) => {
    await gotoHarness(page);
    await row(page, 'bravo.dem').click({ button: 'right' });
    await page.evaluate(() => window.__rerender(['alpha.dem']));
    await item(page, 'Launch Preview').click();
    await expect(page.locator('#result')).toHaveText('');
  });

  test('rows are not Tab stops, so Tab still goes checkbox to Remove', async ({ page }) => {
    await gotoHarness(page);
    const tabIndexes = await page.$$eval('#master-demo-table-body tr', (rs) => rs.map((r) => r.tabIndex));
    expect(tabIndexes.every((t) => t === -1)).toBe(true);
  });
});
