// clip_name_ui.js — Configuration > Render Output's Clip Name Template (#441).
//
// Pins what the user sees while typing a template: the preview built from the
// selected highlight, with the full path; an inline error for a mistake; the
// chips inserting at the cursor (a modifier going inside the placeholder just
// before it); Default putting the default template back; and the chips laid
// out in the same rows as Rename Demos' (#751).
import { test, expect } from '@playwright/test';

async function loadHarness(page) {
  await page.goto('/tests/e2e/clip-name.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

test('the default template previews the selected highlight with its full path', async ({ page }) => {
  await loadHarness(page);
  await expect(page.locator('#config-clip-name-template')).toHaveValue('{map}_{name}_{kills}k_{weapons}_{time}');
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

test('the team placeholders resolve through the Teams list (#445)', async ({ page }) => {
  await loadHarness(page);
  const input = page.locator('#config-clip-name-template');
  const preview = page.locator('#config-clip-name-preview');
  await input.fill('{player}_{team_name}_v_{opponent}_{map}');
  await expect(preview).toContainText('Preview: krod_KROD_v_dice_anzio');
  await expect(page.locator('.clip-name-chip', { hasText: '{opponent}' }))
    .toHaveAttribute('title', /Selected highlight: dice/);
  // An untagged side gives unknown, or the word after |, never the faction.
  await page.evaluate(() => { window.__demo.teams[0].tag = null; });
  await input.fill('{player}_v_{opponent}_{map}');
  await expect(preview).toContainText('Preview: krod_v_unknown_anzio');
  await input.fill('{player}_v_{opponent|mix}_{map}');
  await expect(preview).toContainText('Preview: krod_v_mix_anzio');
});

test('Default puts the default template back and saves', async ({ page }) => {
  await loadHarness(page);
  await page.locator('#config-clip-name-template').fill('{row}');
  await page.locator('#config-clip-name-reset').click();
  await expect(page.locator('#config-clip-name-template')).toHaveValue('{map}_{name}_{kills}k_{weapons}_{time}');
  // Leaving the field saved the typed template; Default saved again.
  expect(await page.evaluate(() => window.__changes)).toBe(2);
});

test('the chips sit in the same rows as Rename Demos, plus Highlight (#751)', async ({ page }) => {
  await loadHarness(page);
  await expect(page.locator('#config-clip-name-chips .placeholder-chip-group-label'))
    .toHaveText(['Player', 'Sides', 'Teams', 'Demo', 'Highlight', 'Format']);
  await expect(page.locator('.placeholder-chip-group[data-group="player"] .clip-name-chip')).toHaveText(['{name}', '{full_name}']);
  await expect(page.locator('.placeholder-chip-group[data-group="highlight"] .clip-name-chip'))
    .toHaveText(['{kills}', '{weapons}', '{first_weapon}', '{victims}', '{first_victim}', '{row}', '{time}']);
  // {kills} counts something else in Rename Demos, and says so.
  await expect(page.locator('.clip-name-chip:text-is("{kills}")')).toHaveAttribute('title', /Rename Demos/);
  await expect(page.locator('.clip-name-chip:text-is("{player}")')).toHaveCount(0);
});

test('an old {player} template still works, and the side teams fill in', async ({ page }) => {
  await loadHarness(page);
  const input = page.locator('#config-clip-name-template');
  await input.fill('{player}_{allies}_v_{axis}_{team1}_{demo_type}_{row}');
  await expect(page.locator('#config-clip-name-errors')).toBeHidden();
  // Allies dicE (shown as dice on the Teams list), Axis KROD: team1 is the
  // alphabetically first of the two display names.
  await expect(page.locator('#config-clip-name-preview')).toContainText('Preview: krod_dice_v_KROD_dice_pov_01');
});
