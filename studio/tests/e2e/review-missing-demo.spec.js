// Review reaching a demo whose file is missing (#1376): a pop-up asks, with
// Locate…, Use found copy (when a copy was found), Remove and Skip and keep
// reviewing; closing it cancels the review. Driven against
// tests/e2e/review-missing-demo.html. See tests/e2e/README.md.
import { test, expect } from '@playwright/test';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/review-missing-demo.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const modal = (page) => page.locator('#themed-confirm-modal');
const choice = (page, label) => modal(page).getByRole('button', { name: label, exact: true });
const queue = (page) => page.locator('#queue li');
const sent = (page) => page.evaluate(() => window.__sent());
const goneFromDisk = (page, name) => page.evaluate((n) => window.__disk.delete(`D:/tests/${n}`), name);
const review = (page) => page.click('#review-highlights-btn');

test.describe('Review with a missing demo', () => {
  test('asks with Locate, Remove and Skip, and with Use found copy only when a copy was found', async ({ page }) => {
    await gotoHarness(page);
    await goneFromDisk(page, 'a.dem');
    await review(page);
    await expect(modal(page)).toBeVisible();
    await expect(modal(page).locator('h2')).toHaveText('Demo not found');
    await expect(modal(page)).toContainText('a.dem');
    await expect(modal(page).locator('button:visible')).toHaveText(['Locate…', 'Remove', 'Skip and keep reviewing', 'Cancel review']);
  });

  test('offers Use found copy when a copy was found', async ({ page }) => {
    await gotoHarness(page);
    await goneFromDisk(page, 'a.dem');
    await page.evaluate(() => { window.__renamed['D:/tests/a.dem'] = 'D:/tests/ax.dem'; });
    await review(page);
    await expect(modal(page).locator('button:visible')).toHaveText(['Locate…', 'Use found copy', 'Remove', 'Skip and keep reviewing', 'Cancel review']);
  });

  test('Skip and keep reviewing reviews the others, keeps the row and logs it', async ({ page }) => {
    await gotoHarness(page);
    await goneFromDisk(page, 'a.dem');
    await review(page);
    await choice(page, 'Skip and keep reviewing').click();
    await expect(modal(page)).toBeHidden();
    await expect.poll(() => sent(page)).toEqual([['D:/tests/b.dem', 'D:/tests/c.dem']]);
    await expect(queue(page)).toHaveText(['a.dem (missing)', 'b.dem', 'c.dem']);
    const logged = await page.evaluate(() => window.__logged());
    expect(logged.filter((l) => l.includes('skipped a.dem'))).toHaveLength(1);
  });

  test('closing the pop-up cancels the review: nothing is sent', async ({ page }) => {
    await gotoHarness(page);
    await goneFromDisk(page, 'a.dem');
    await review(page);
    await choice(page, 'Cancel review').click();
    await expect(modal(page)).toBeHidden();
    expect(await sent(page)).toEqual([]);
    await expect(page.locator('#review-highlights-btn')).toBeEnabled();
  });

  test('Use found copy points the row at the copy and reviews it', async ({ page }) => {
    await gotoHarness(page);
    await goneFromDisk(page, 'a.dem');
    await page.evaluate(() => { window.__renamed['D:/tests/a.dem'] = 'D:/tests/ax.dem'; });
    await review(page);
    await choice(page, 'Use found copy').click();
    await expect.poll(() => sent(page)).toEqual([['D:/tests/ax.dem', 'D:/tests/b.dem', 'D:/tests/c.dem']]);
    await expect(queue(page).first()).toHaveText('ax.dem');
  });

  test('Remove takes the row out of the queue and reviews the rest', async ({ page }) => {
    await gotoHarness(page);
    await goneFromDisk(page, 'b.dem');
    await review(page);
    await choice(page, 'Remove').click();
    await expect.poll(() => sent(page)).toEqual([['D:/tests/a.dem', 'D:/tests/c.dem']]);
    await expect(queue(page)).toHaveText(['a.dem', 'c.dem']);
  });

  test('asks about each missing demo, one after the other', async ({ page }) => {
    await gotoHarness(page);
    await goneFromDisk(page, 'a.dem');
    await goneFromDisk(page, 'c.dem');
    await review(page);
    await expect(modal(page)).toContainText('a.dem');
    await choice(page, 'Skip and keep reviewing').click();
    await expect(modal(page)).toContainText('c.dem');
    await choice(page, 'Skip and keep reviewing').click();
    await expect.poll(() => sent(page)).toEqual([['D:/tests/b.dem']]);
  });

  test('says the search stopped early when it did, and to use Locate', async ({ page }) => {
    await gotoHarness(page);
    await goneFromDisk(page, 'a.dem');
    await page.evaluate(() => { window.__stopped.push('D:/tests/a.dem'); });
    await review(page);
    await expect(modal(page)).toContainText('stopped early');
    await expect(modal(page)).toContainText('Use Locate…');
  });
});
