// demo_rename_ui.js — the Demo Auditor's Rename Demos panel (#469).
//
// Pins the preview built from the listed demos and both templates, that
// unticking and template errors keep demos out of the batch, that Rename
// sends only the renames (after a confirm) and the list follows them, that
// Undo puts them back, and that a settled template is saved.
import { test, expect } from '@playwright/test';

// native::demo_rename::DemoFacts, as serde sends it.
const MODIFIED = new Date(2026, 8, 28, 12).getTime() / 1000;
const facts = (fileName, extra = {}) => ({
  path: `C:\\demos\\${fileName}`, file_name: fileName, map: 'dod_anzio', modified_unix_secs: MODIFIED,
  demo_type: 'pov', name: 'krod', kills: 31, deaths: 12, side: 'Axis',
  teams: [{ side: 'Allies', tag: 'dicE' }, { side: 'Axis', tag: 'gskiLL' }], error: null, ...extra,
});
const FACTS = [
  facts('pov1.dem'),
  facts('hltv1.dem', { demo_type: 'hltv', name: null, kills: null, deaths: null, side: null }),
  facts('broken.dem', { error: 'no frames', map: null, teams: [] }),
];

async function loadHarness(page, { undoable = null } = {}) {
  await page.addInitScript(({ list, undo }) => {
    window.__mockInvokeHandlers = window.__mockInvokeHandlers || {};
    window.__mockInvokeHandlers.demo_rename_list = () => list;
    window.__mockInvokeHandlers.demo_rename_undoable = () => window.__undoable ?? undo;
    window.__mockInvokeHandlers.demo_rename_apply = ({ renames }) => {
      window.__undoable = { log: 'x', count: renames.length, created_unix_secs: 1 };
      return { renamed: renames, failed: [], log: 'x' };
    };
    window.__mockInvokeHandlers.demo_rename_undo = () => {
      window.__undoable = null;
      return { renamed: [{ from: 'C:\\demos\\krod_31k_v_dicE_anzio.dem', to: 'C:\\demos\\pov1.dem' }], failed: [], log: 'x' };
    };
  }, { list: FACTS, undo: undoable });
  await page.goto('/tests/e2e/demo-rename.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const newNames = (page) => page.locator('#rename-body tr td:nth-child(3)');

test('List Demos previews each type from its own template', async ({ page }) => {
  await loadHarness(page);
  await expect(page.locator('#rename-body td')).toHaveText('Choose a Target Folder above and click List Demos.');
  await page.click('#rename-list-btn');

  await expect(page.locator('#rename-body tr td:nth-child(2)')).toHaveText(['pov1.dem', 'hltv1.dem', 'broken.dem']);
  await expect(newNames(page)).toHaveText([
    'krod_31k_v_dicE_anzio.dem',
    'dicE_v_gskiLL_anzio_2026-09-28.dem',
    "couldn't be read: no frames",
  ]);
  await expect(page.locator('#rename-body tr td:nth-child(4)')).toHaveText(['POV', 'HLTV', 'POV']);
  await expect(page.locator('#rename-status')).toHaveText('3 demos, 2 to rename.');
  await expect(page.locator('#rename-apply-btn')).toHaveText('Rename 2 Demos');
  await expect(page.locator('#rename-apply-btn')).toBeEnabled();
});

test('unticking a demo, or a template with errors, keeps it out', async ({ page }) => {
  await loadHarness(page);
  await page.click('#rename-list-btn');
  await page.locator('#rename-body tr').nth(0).locator('input').uncheck();
  await expect(newNames(page).nth(0)).toHaveText('not ticked');
  await expect(page.locator('#rename-select-all')).not.toBeChecked();

  await page.fill('#rename-hltv-template', '{row}_{map}');
  await expect(page.locator('#rename-hltv-errors')).toContainText('{row}');
  await expect(newNames(page).nth(1)).toHaveText('its template has a problem (see above)');
  await expect(page.locator('#rename-apply-btn')).toBeDisabled();

  await page.click('#rename-hltv-reset');
  await expect(page.locator('#rename-hltv-errors')).toBeHidden();
  await expect(page.locator('#rename-apply-btn')).toHaveText('Rename 1 Demo');
});

test('a chip goes into the template field last used, and a settled template is saved', async ({ page }) => {
  await loadHarness(page);
  await page.click('#rename-list-btn');
  await page.fill('#rename-pov-template', '{map}_');
  await page.locator('#rename-pov-template').press('End');
  await page.click('.rename-chip:text-is("{name}")');
  await expect(page.locator('#rename-pov-template')).toHaveValue('{map}_{name}');
  await expect(newNames(page).nth(0)).toHaveText('anzio_krod.dem');
  expect(await page.evaluate(() => window.__changes)).toBeGreaterThan(0);
});

test('the |fallback chip goes inside the placeholder, ready for the word', async ({ page }) => {
  await loadHarness(page);
  await page.click('#rename-list-btn');
  await page.fill('#rename-pov-template', '{team_name}');
  await page.locator('#rename-pov-template').press('End');
  const chip = page.locator('.rename-chip:text-is("|fallback")');
  // The tooltip names every placeholder it works on.
  await expect(chip).toHaveAttribute('title', /\{name\} \{full_name\} \{faction\} \{enemy_faction\} \{team_name\} \{opponent\} \{allies\} \{axis\} \{team1\} \{team2\}/);
  await chip.click();
  await page.keyboard.type('mix');
  await expect(page.locator('#rename-pov-template')).toHaveValue('{team_name|mix}');
  await expect(page.locator('#rename-pov-errors')).toBeHidden();
  await expect(page.locator('.rename-chip:text-is("{team_name}")')).toHaveAttribute('title', /your word after \|/);
  await expect(page.locator('.rename-chip:text-is("{map}")')).not.toHaveAttribute('title', /your word after \|/);
});

test('Lowercase the whole name lower-cases every new name and is saved', async ({ page }) => {
  await loadHarness(page);
  await page.click('#rename-list-btn');
  const before = await page.evaluate(() => window.__changes);
  await page.check('#rename-lowercase');
  await expect(newNames(page).nth(0)).toHaveText('krod_31k_v_dice_anzio.dem');
  await expect(newNames(page).nth(1)).toHaveText('dice_v_gskill_anzio_2026-09-28.dem');
  expect(await page.evaluate(() => window.__changes)).toBeGreaterThan(before);
});

test('the HLTV template keeps every chip but greys out the POV-only ones', async ({ page }) => {
  await loadHarness(page);
  await page.click('#rename-list-btn');
  const faction = page.locator('.rename-chip:text-is("{faction}")');
  await page.locator('#rename-pov-template').focus();
  await expect(faction).toBeEnabled();
  await page.locator('#rename-hltv-template').focus();
  await expect(faction).toBeVisible();
  await expect(faction).toBeDisabled();
  await expect(faction).toHaveAttribute('title', /only works in the POV template/);
  await expect(page.locator('.rename-chip:text-is("{allies}")')).toBeEnabled();
  // One labelled row per group, the modifiers (with :first) last.
  await expect(page.locator('.rename-chip-group-label')).toHaveText(['Player', 'Sides', 'Teams', 'Demo', 'Format']);
  await expect(page.locator('.rename-chip-group[data-group="player"] .rename-chip:disabled')).toHaveCount(4);
  await expect(page.locator('.rename-chip-group[data-group="format"] .rename-chip')).toHaveText([':lower', ':upper', ':first', '|fallback']);

  await page.fill('#rename-hltv-template', '{kills}_{map}');
  await expect(page.locator('#rename-hltv-errors')).toContainText('only works in the POV template');
  await expect(newNames(page).nth(1)).toHaveText('its template has a problem (see above)');
});

test('Rename confirms, sends only the renames, follows them, and Undo puts them back', async ({ page }) => {
  await loadHarness(page);
  await page.click('#rename-list-btn');
  await page.click('#rename-apply-btn');
  await expect(page.locator('#themed-confirm-title')).toHaveText('Rename 2 demos?');
  await page.click('#themed-confirm-ok-btn');

  const sent = await page.evaluate(() => window.__mockInvocations.find((c) => c.cmd === 'demo_rename_apply').args.renames);
  expect(sent).toEqual([
    { from: 'C:\\demos\\pov1.dem', to: 'C:\\demos\\krod_31k_v_dicE_anzio.dem' },
    { from: 'C:\\demos\\hltv1.dem', to: 'C:\\demos\\dicE_v_gskiLL_anzio_2026-09-28.dem' },
  ]);
  // The list now holds the new names, which the templates already give.
  await expect(page.locator('#rename-body tr td:nth-child(2)')).toHaveText(
    ['krod_31k_v_dicE_anzio.dem', 'dicE_v_gskiLL_anzio_2026-09-28.dem', 'broken.dem']);
  await expect(newNames(page).nth(0)).toHaveText('already named this way');
  await expect(page.locator('#rename-status')).toHaveText('3 demos, 0 to rename.');
  await expect(page.locator('#rename-undo-btn')).toBeEnabled();

  await page.click('#rename-undo-btn');
  await expect(page.locator('#rename-body tr td:nth-child(2)').nth(0)).toHaveText('pov1.dem');
  await expect(page.locator('#rename-undo-btn')).toBeDisabled();
});

test('Undo is ready at start when an earlier batch can be put back', async ({ page }) => {
  await loadHarness(page, { undoable: { log: 'x', count: 4, created_unix_secs: MODIFIED } });
  await expect(page.locator('#rename-undo-btn')).toBeEnabled();
  await expect(page.locator('#rename-undo-btn')).toHaveAttribute('title', /Put back the 4 demos renamed/);
});

test('List Demos shows a bar from the first moment, counting cached demos first', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => {
    window.__mockInvokeHandlers.demo_rename_list = () => new Promise((resolve) => { window.__finishList = resolve; });
  });
  await page.click('#rename-list-btn');
  const bar = page.locator('#rename-progress');
  // The harness has no stylesheet, so the bar has no height: check hidden.
  await expect(bar).toHaveJSProperty('hidden', false);
  await expect(page.locator('#rename-status')).toHaveText('Checking the analyzer cache…');
  await page.evaluate(() => window.__mockEmit('demo_rename_progress', { done: 5, total: 6, cached: 5, parsed: 0 }));
  await expect(page.locator('#rename-status')).toHaveText('Reading demos: 5 / 6 (5 from the analyzer cache)');
  await expect(bar.locator('.progress-bar-fill')).toHaveAttribute('style', /width: 83%/);
  await page.evaluate(() => window.__finishList([]));
  await expect(bar).toHaveJSProperty('hidden', true);
});
