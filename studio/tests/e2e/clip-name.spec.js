// clip_name_ui.js — Configuration > Render Output's Clip Name Template (#441).
//
// Pins what the user sees while typing a template: the preview built from the
// selected highlight, with the full path; an inline error for a mistake; the
// chips inserting at the cursor (a modifier going inside the placeholder just
// before it); and Default putting the default template back.
import { test, expect } from '@playwright/test';

async function loadHarness(page) {
  await page.goto('/tests/e2e/clip-name.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

test('the default template previews the selected highlight with its full path', async ({ page }) => {
  await loadHarness(page);
  await expect(page.locator('#config-clip-name-template')).toHaveValue('{map}_{player}_{kills}k_{weapons}_{time}');
  const preview = page.locator('#config-clip-name-preview');
  await expect(preview).toContainText('Preview: anzio_krod_2k_garand-mp40_12m34s');
  await expect(preview).toContainText('D:\\Clips\\anzio_krod_2k_garand-mp40_12m34s (41 characters)');
  await expect(page.locator('#config-clip-name-errors')).toBeHidden();
});

test('a typo shows an error as you type, shown as typed in the preview', async ({ page }) => {
  await loadHarness(page);
  const input = page.locator('#config-clip-name-template');
  await input.fill('{oponent}_{row}');
  await expect(page.locator('#config-clip-name-errors')).toBeVisible();
  await expect(page.locator('#config-clip-name-errors')).toContainText("{oponent} isn't a placeholder");
  await expect(page.locator('#config-clip-name-preview')).toContainText('Preview: {oponent}_01');
  expect(await page.evaluate(() => window.__errors().length)).toBe(1);
});

test('chips insert at the cursor, and a modifier lands inside the placeholder', async ({ page }) => {
  await loadHarness(page);
  const input = page.locator('#config-clip-name-template');
  await input.fill('{faction}');
  await input.evaluate((el) => el.setSelectionRange(el.value.length, el.value.length));
  await page.locator('.clip-name-chip', { hasText: ':lower' }).click();
  await expect(input).toHaveValue('{faction:lower}');
  await page.locator('.clip-name-chip', { hasText: '{row}' }).click();
  await expect(input).toHaveValue('{faction:lower}{row}');
  await expect(page.locator('#config-clip-name-preview')).toContainText('Preview: axis01');
  expect(await page.evaluate(() => window.__changes)).toBe(2);
});

test('a chip tooltip shows its value for the selected highlight', async ({ page }) => {
  await loadHarness(page);
  await expect(page.locator('.clip-name-chip', { hasText: '{enemy_faction}' }))
    .toHaveAttribute('title', /Selected highlight: Allies/);
});

test('Default puts the default template back and saves', async ({ page }) => {
  await loadHarness(page);
  await page.locator('#config-clip-name-template').fill('{row}');
  await page.locator('#config-clip-name-reset').click();
  await expect(page.locator('#config-clip-name-template')).toHaveValue('{map}_{player}_{kills}k_{weapons}_{time}');
  // Leaving the field saved the typed template; Default saved again.
  expect(await page.evaluate(() => window.__changes)).toBe(2);
});
