// Highlight Details' Notes column (#605) -- driven against
// tests/e2e/detail-notes.html. See tests/e2e/README.md.
import { test, expect } from '@playwright/test';

async function gotoHarness(page) {
  await page.goto('/tests/e2e/detail-notes.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const notes = (page) => page.locator('#detail-streaks-table .streak-notes-input');

test.describe('Highlight Details notes', () => {
  test('a note is a wrapping, resizable box showing the text exactly', async ({ page }) => {
    await gotoHarness(page);
    const first = notes(page).first();
    await expect(first).toHaveJSProperty('tagName', 'TEXTAREA');
    await expect(first).toHaveValue('nice "flick" <then> a 2k through the door, long enough to wrap over a second line');
    await expect(first).toHaveCSS('resize', 'both');
    await expect(notes(page).nth(1)).toHaveValue('');
    await expect(notes(page).nth(1)).toHaveAttribute('placeholder', 'Add note...');
  });

  test('the Notes heading is found by its class and takes the room', async ({ page }) => {
    await gotoHarness(page);
    const th = page.locator('#detail-streaks-table th.col-notes');
    await expect(th).toHaveText('Notes');
    const width = await th.evaluate((el) => el.getBoundingClientRect().width);
    expect(width).toBeGreaterThan(300);
  });

  test('typing updates the highlight, and a newline stays in the note', async ({ page }) => {
    await gotoHarness(page);
    const second = notes(page).nth(1);
    await second.click();
    await second.type('first line');
    await second.press('Enter');
    await second.type('second line');
    expect(await page.evaluate(() => window.__demo.streaks[1].notes)).toBe('first line\nsecond line');
  });
});
