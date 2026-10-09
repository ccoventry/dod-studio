// teams_pane.js (#445) — driven against tests/e2e/teams.html.
// See tests/e2e/README.md for what this style of test covers.
import { test, expect } from '@playwright/test';

async function openTeams(page) {
  await page.goto('/tests/e2e/teams.html');
  await page.waitForFunction(() => window.__harnessReady === true);
  await page.locator('#teams-btn').click();
  await expect(page.locator('#teams-modal')).toBeVisible();
}

const row = (page, tag) => page.locator(`#teams-table-body tr[data-tag="${tag}"]`);

test.describe('Teams list', () => {
  test('lists each tag with its demo count, most demos first', async ({ page }) => {
    await openTeams(page);
    const tags = await page.locator('#teams-table-body tr').evaluateAll((trs) => trs.map((tr) => tr.dataset.tag));
    expect(tags[0]).toBe('dicE');
    expect(tags).toHaveLength(5);
    await expect(row(page, 'dicE').locator('.teams-count')).toHaveText('2');
    await expect(row(page, 'dicE').locator('.teams-count')).toHaveAttribute('title', 'a.dem\nb.dem');
  });

  test('a tag from a player name is shown as text, not markup', async ({ page }) => {
    await openTeams(page);
    await expect(row(page, '<b>x</b>').locator('td').first()).toHaveText('<b>x</b>');
    await expect(page.locator('#teams-table-body b')).toHaveCount(0);
  });

  test('typing a name saves it against the tag and marks the project changed', async ({ page }) => {
    await openTeams(page);
    const input = row(page, 'dicE').locator('.teams-name-input');
    await expect(input).toHaveAttribute('placeholder', 'dicE');
    await input.fill('Dice');
    await input.press('Enter');
    expect(await page.evaluate(() => window.__projectTeams.names)).toEqual({ dicE: 'Dice' });
    expect(await page.evaluate(() => window.__changes)).toBe(1);
    await expect(row(page, 'dicE').locator('.teams-name-input')).toHaveValue('Dice');
  });

  test('merging folds a tag into another row, and × splits it back out', async ({ page }) => {
    await openTeams(page);
    await row(page, 'jover').locator('.teams-same-select').selectOption('over');
    await expect(row(page, 'jover')).toHaveCount(0);
    await expect(row(page, 'over').locator('.teams-count')).toHaveText('2');
    await expect(row(page, 'over').locator('.teams-also')).toContainText('also jover');
    expect(await page.evaluate(() => window.__projectTeams.merged)).toEqual({ jover: 'over' });

    await row(page, 'over').locator('.teams-also button').click();
    await expect(row(page, 'jover')).toHaveCount(1);
    expect(await page.evaluate(() => window.__projectTeams.merged)).toEqual({});
    expect(await page.evaluate(() => window.__changes)).toBe(2);
  });

  test('demos scanned before teams were read can be read from here', async ({ page }) => {
    await openTeams(page);
    await expect(page.locator('#teams-unread-row')).toBeVisible();
    await expect(page.locator('#teams-unread-note')).toContainText('1 demo(s)');
    await page.locator('#teams-read-btn').click();
    await expect(page.locator('#teams-unread-row')).toBeHidden();
    expect(await page.evaluate(() => window.__readPaths)).toEqual(['C:\\demos\\old.dem']);
    await expect(row(page, 'krod')).toHaveCount(1);
    await expect(row(page, 'dicE').locator('.teams-count')).toHaveText('3');
  });

  test('Close hides the modal', async ({ page }) => {
    await openTeams(page);
    await page.locator('#teams-close-btn').click();
    await expect(page.locator('#teams-modal')).toBeHidden();
  });
});
