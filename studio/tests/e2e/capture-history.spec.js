// Highlight Details' capture history (#439) -- driven against
// tests/e2e/capture-history.html. See tests/e2e/README.md.
import { test, expect } from '@playwright/test';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/capture-history.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const toggles = (page) => page.locator('#detail-streaks-table .capture-history-toggle');
const lines = (page) => page.locator('.capture-history-line');

test.describe('Highlight Details capture history', () => {
  test('a highlight with captures gets an arrow with a tooltip, and the list starts closed', async ({ page }) => {
    await gotoHarness(page);
    await expect(toggles(page)).toHaveCount(2);
    await expect(toggles(page).first()).toHaveAttribute('title', /every time this highlight was captured/);
    await expect(lines(page)).toHaveCount(0);
  });

  test('expanding lists every capture newest first, a deleted take dimmed, a shared take named', async ({ page }) => {
    await gotoHarness(page);
    await toggles(page).first().click();
    await expect(lines(page)).toHaveCount(3);
    await expect(lines(page).nth(0)).toHaveAttribute('data-take-key', 'live_session/chain_01_b1');
    await expect(lines(page).nth(1)).toHaveAttribute('data-take-key', 'gone_session/chain_00_b0');
    await expect(lines(page).nth(2)).toHaveAttribute('data-take-key', 'old_session/chain_00_b0');

    const live = lines(page).nth(0);
    await expect(live).toContainText('Video · 120 fps');
    await expect(live).toContainText('Shared with Bob at 2:00');
    await expect(live.getByRole('button', { name: 'Open Folder' })).toBeVisible();

    const gone = lines(page).nth(1);
    await expect(gone).toContainText('Frame sequence · 300 fps');
    await expect(gone).toContainText('take deleted');
    await expect(gone).toHaveCSS('opacity', '0.5');
    await expect(gone.getByRole('button', { name: 'Open Folder' })).toHaveCount(0);

    const old = lines(page).nth(2);
    await expect(old).toContainText('Captured earlier (date not recorded)');
    await expect(old).toContainText('folder not recorded');
  });

  test('the renders of a take sit under it, with a button only for a video that is there', async ({ page }) => {
    await gotoHarness(page);
    await toggles(page).first().click();
    const renders = lines(page).nth(0).locator('.capture-history-render');
    await expect(renders).toHaveCount(2);
    await expect(renders.nth(0)).toContainText('Failed: codec not found');
    await expect(renders.nth(0).getByRole('button')).toHaveCount(0);
    await expect(renders.nth(1)).toContainText('live.mov');
    await renders.nth(1).getByRole('button', { name: 'Show video' }).click();
    expect(await page.evaluate(() => window.__revealed)).toEqual(['D:/out/live.mov']);
  });

  test('Open Folder shows the take folder, and a second click on the arrow closes the list', async ({ page }) => {
    await gotoHarness(page);
    await toggles(page).first().click();
    await lines(page).nth(0).getByRole('button', { name: 'Open Folder' }).click();
    expect(await page.evaluate(() => window.__revealed)).toEqual(['D:/caps/live_session/chain_01_b1']);
    await toggles(page).first().click();
    await expect(lines(page)).toHaveCount(0);
  });

  test('the second highlight sees only the take it shares, and an open list survives a re-render', async ({ page }) => {
    await gotoHarness(page);
    await toggles(page).nth(1).click();
    await expect(lines(page)).toHaveCount(1);
    await expect(lines(page).first()).toContainText('Shared with Ann at 1:00');
    await page.evaluate(async () => {
      const { renderDetailView } = await import('/src/detail_pane.js');
      renderDetailView(window.__demo, 0);
    });
    await expect(lines(page)).toHaveCount(1);
  });
});
