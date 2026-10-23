// searchable_select.js — a dropdown you can type into (Configuration > Paths >
// GoldSrc Hooks DLL).
//
// Pins: a click opens the whole list with the picked row marked; typing
// filters it; a click or Enter picks, closes the list and shows the picked
// text instead of what was typed; Esc and leaving the box put the picked text
// back without a pick; arrows move through the list.
import { test, expect } from '@playwright/test';

async function loadHarness(page) {
  await page.goto('/tests/e2e/searchable-select.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const box = (page) => page.locator('#box');
const panel = (page) => page.locator('.search-select-panel');
const rows = (page) => page.locator('.search-select-option');
const picks = (page) => page.evaluate(() => window.__picks);

test('shows the picked text, and a click opens the whole list with it marked', async ({ page }) => {
  await loadHarness(page);
  await expect(box(page)).toHaveValue('889 · Oct 22 17:54');
  await expect(panel(page)).toBeHidden();
  await box(page).click();
  await expect(panel(page)).toBeVisible();
  await expect(rows(page)).toHaveCount(5);
  await expect(page.locator('.search-select-option.picked')).toHaveText('889 · Oct 22 17:54');
});

test('typing filters the list, and clicking a row picks it and shows its text', async ({ page }) => {
  await loadHarness(page);
  await box(page).click();
  await page.keyboard.type('91');
  await expect(rows(page)).toHaveText(['910 · Oct 22 17:54', '918 · Oct 22 01:10']);
  await rows(page).nth(1).click();
  await expect(panel(page)).toBeHidden();
  await expect(box(page)).toHaveValue('918 · Oct 22 01:10');
  expect(await picks(page)).toEqual(['p918']);
});

test('Enter picks the highlighted row: the first match, or where the arrows moved', async ({ page }) => {
  await loadHarness(page);
  await box(page).click();
  await page.keyboard.type('91');
  await page.keyboard.press('Enter');
  await expect(box(page)).toHaveValue('910 · Oct 22 17:54');
  await box(page).click();
  await page.keyboard.type('Oct 22');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  expect(await picks(page)).toEqual(['p910', 'p918']);
});

test('Esc or leaving the box puts the picked text back and picks nothing', async ({ page }) => {
  await loadHarness(page);
  await box(page).click();
  await page.keyboard.type('91');
  await page.keyboard.press('Escape');
  await expect(panel(page)).toBeHidden();
  await expect(box(page)).toHaveValue('889 · Oct 22 17:54');
  await page.keyboard.type('55');
  await page.locator('#after').click();
  await expect(box(page)).toHaveValue('889 · Oct 22 17:54');
  expect(await picks(page)).toEqual([]);
});

test('typing what matches nothing says so', async ({ page }) => {
  await loadHarness(page);
  await box(page).click();
  await page.keyboard.type('zzz');
  await expect(rows(page)).toHaveCount(0);
  await expect(page.locator('.search-select-empty')).toHaveText('No test DLL matches');
});
