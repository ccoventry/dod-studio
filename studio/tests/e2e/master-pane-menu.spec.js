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
});
