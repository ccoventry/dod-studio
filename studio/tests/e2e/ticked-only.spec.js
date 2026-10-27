// The Master Queue's Ticked only filter (#127), driven against
// tests/e2e/ticked-only.html. See tests/e2e/README.md.
import { test, expect } from '@playwright/test';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/ticked-only.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const rows = (page) => page.locator('#master-demo-table-body tr');
const toggle = (page) => page.locator('#master-ticked-only-cb');
const label = (page) => page.locator('.queue-filter-ticked-only');

test.describe('Ticked only', () => {
  test('hides the demos with no highlight ticked, and shows them again when turned off', async ({ page }) => {
    await gotoHarness(page);
    await expect(rows(page)).toHaveCount(3);
    await toggle(page).check();
    await expect(rows(page)).toHaveCount(1);
    await expect(rows(page)).toContainText('ticked.dem');
    await toggle(page).uncheck();
    await expect(rows(page)).toHaveCount(3);
  });

  test('a demo shows up once one of its highlights is ticked', async ({ page }) => {
    await gotoHarness(page);
    await toggle(page).check();
    await page.evaluate(() => window.__tick('other.dem', true));
    await expect(rows(page)).toHaveCount(2);
    await expect(rows(page).filter({ hasText: 'other.dem' })).toHaveCount(1);
  });

  test('the demo open in Highlight Details stays listed when its last tick goes', async ({ page }) => {
    await gotoHarness(page);
    await rows(page).filter({ hasText: 'ticked.dem' }).click();
    await toggle(page).check();
    await page.evaluate(() => window.__tick('ticked.dem', false));
    await expect(rows(page)).toHaveCount(1);
    await expect(rows(page)).toContainText('ticked.dem');
  });

  test('is dimmed with a reason while nothing is ticked, and says what it does once something is', async ({ page }) => {
    await gotoHarness(page);
    await expect(toggle(page)).toBeEnabled();
    await expect(label(page)).toHaveAttribute('title', /Hide demos with no highlight ticked/);
    await page.evaluate(() => window.__untickAll());
    await expect(toggle(page)).toBeDisabled();
    await expect(label(page)).toHaveClass(/is-disabled/);
    await expect(label(page)).toHaveAttribute('title', /No highlight is ticked yet/);
    await page.evaluate(() => window.__tick('plain.dem', true));
    await expect(toggle(page)).toBeEnabled();
    await expect(label(page)).not.toHaveClass(/is-disabled/);
  });

  test('once on, it stays clickable after the last tick goes, so it can be turned off', async ({ page }) => {
    await gotoHarness(page);
    await toggle(page).check();
    await page.evaluate(() => window.__untickAll());
    await expect(rows(page)).toContainText('No demos match your search and filters.');
    await expect(toggle(page)).toBeEnabled();
    await toggle(page).uncheck();
    await expect(rows(page)).toHaveCount(3);
    await expect(toggle(page)).toBeDisabled();
  });
});
