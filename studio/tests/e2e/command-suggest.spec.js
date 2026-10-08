// command_suggest.js — the Commands tab's type-ahead (#215).
//
// Pins the list appearing as a name is typed, keyboard and mouse picking,
// that the rest of a line survives, and that a name Studio refuses says so
// in the list for the list it would be refused in.
import { test, expect } from '@playwright/test';

async function loadHarness(page) {
  await page.goto('/tests/e2e/command-suggest.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const items = (page) => page.locator('.command-suggest .command-suggest-name');

test('typing lists the names it could be; Down then Enter takes one', async ({ page }) => {
  await loadHarness(page);
  await page.locator('#init-input').pressSequentially('mirv_movie_f');
  await expect(items(page).first()).toBeVisible();
  const names = await items(page).allTextContents();
  expect(names.every((n) => n.startsWith('mirv_movie_f'))).toBe(true);
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  await expect(page.locator('#init-input')).toHaveValue(`${names[1]} `);
  await expect(page.locator('.command-suggest')).toHaveCount(0);
});

test('a click takes a name and keeps the value already typed', async ({ page }) => {
  await loadHarness(page);
  const input = page.locator('#init-input');
  await input.fill('fps 300');
  await input.evaluate((el) => { el.setSelectionRange(3, 3); el.dispatchEvent(new Event('input')); });
  await page.locator('.command-suggest-item', { hasText: 'fps_max' }).first().dispatchEvent('mousedown');
  await expect(input).toHaveValue('fps_max 300');
});

test('a refused name says so, and only in the list that refuses it', async ({ page }) => {
  await loadHarness(page);
  await page.locator('#scheduled-input').pressSequentially('r_decal');
  const scheduled = page.locator('.command-suggest-item', { hasText: 'r_decals' });
  await expect(scheduled).toHaveClass(/command-suggest-refused/);
  await expect(scheduled).toContainText('Initial Commands only');
  await page.keyboard.press('Escape');
  await expect(page.locator('.command-suggest')).toHaveCount(0);

  await page.locator('#init-input').pressSequentially('r_decal');
  const init = page.locator('.command-suggest-item', { hasText: 'r_decals' });
  await expect(init).toHaveClass(/command-suggest-warned/);
});

test('no list once a space has been typed after the name', async ({ page }) => {
  await loadHarness(page);
  await page.locator('#init-input').pressSequentially('mirv_fov ');
  await expect(page.locator('.command-suggest')).toHaveCount(0);
});
