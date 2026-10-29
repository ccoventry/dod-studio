// The Master Queue's Tracked only filter (#127), driven against
// tests/e2e/tracked-only.html. See tests/e2e/README.md.
import { test, expect } from '@playwright/test';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/tracked-only.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const rows = (page) => page.locator('#master-demo-table-body tr');
const toggle = (page) => page.locator('#master-tracked-only-cb');
const label = (page) => page.locator('.queue-filter-tracked-only');

test.describe('Tracked only', () => {
  test('hides the untracked demos, a ticked one too, and shows them again when turned off', async ({ page }) => {
    await gotoHarness(page);
    await expect(rows(page)).toHaveCount(3);
    await toggle(page).check();
    await expect(rows(page)).toHaveCount(1);
    await expect(rows(page)).toContainText('noted.dem');
    await toggle(page).uncheck();
    await expect(rows(page)).toHaveCount(3);
  });

  test('a demo shows up once one of its highlights gets a note', async ({ page }) => {
    await gotoHarness(page);
    await toggle(page).check();
    await page.evaluate(() => window.__note('other.dem', 'look again'));
    await expect(rows(page)).toHaveCount(2);
    await expect(rows(page).filter({ hasText: 'other.dem' })).toHaveCount(1);
  });

  test('the demo open in Highlight Details stays listed when its last note goes', async ({ page }) => {
    await gotoHarness(page);
    await rows(page).filter({ hasText: 'noted.dem' }).click();
    await toggle(page).check();
    await page.evaluate(() => window.__note('noted.dem', ''));
    await expect(rows(page)).toHaveCount(1);
    await expect(rows(page)).toContainText('noted.dem');
  });

  test('is dimmed with a reason while nothing is tracked, and says what it does once something is', async ({ page }) => {
    await gotoHarness(page);
    await expect(toggle(page)).toBeEnabled();
    await expect(label(page)).toHaveAttribute('title', /Show only the demos with the bookmark/);
    await page.evaluate(() => window.__clearAll());
    await expect(toggle(page)).toBeDisabled();
    await expect(label(page)).toHaveClass(/is-disabled/);
    await expect(label(page)).toHaveAttribute('title', /No demo is tracked yet/);
    await page.evaluate(() => window.__note('plain.dem', 'maybe'));
    await expect(toggle(page)).toBeEnabled();
    await expect(label(page)).not.toHaveClass(/is-disabled/);
  });

  test('once on, it stays clickable after the last tracked demo goes, so it can be turned off', async ({ page }) => {
    await gotoHarness(page);
    await toggle(page).check();
    await page.evaluate(() => window.__clearAll());
    await expect(rows(page)).toContainText('No demos match your search and filters.');
    await expect(toggle(page)).toBeEnabled();
    await toggle(page).uncheck();
    await expect(rows(page)).toHaveCount(3);
    await expect(toggle(page)).toBeDisabled();
  });
});
