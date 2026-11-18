// vegas_relink_export.js: the Render tab's VEGAS relink list (#1514).
//
// Pins what the two buttons save (every old path a render had, to where it is
// now; with a backup, its paths there too), that the backup is only read, and
// that a cancelled save writes nothing.
import { test, expect } from './fixtures.js';

const SAVE_TO = 'H:\\Movie\\dod_highlights.relink.json';
const BACKUP = 'H:\\Movie\\project.dodstudio.backup-before-move';

async function loadHarness(page, { save = SAVE_TO, backupText = null, picked = BACKUP } = {}) {
  await page.addInitScript((h) => {
    window.__mockInvokeHandlers = window.__mockInvokeHandlers || {};
    window.__mockInvokeHandlers.files_exist = (args) => (args?.paths || []).map(() => true);
    window.__mockInvokeHandlers.save_project_session = () => null;
    window.__mockInvokeHandlers.load_project_session = () => h.backupText;
    window.__mockInvokeHandlers['plugin:dialog|open'] = () => h.picked;
    window.__mockInvokeHandlers['plugin:dialog|save'] = () => h.save;
  }, { save, backupText, picked });
  await page.goto('/tests/e2e/vegas-relink.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const calls = (page, cmd) => page.evaluate(
  (c) => window.__mockInvocations.filter((i) => i.cmd === c).map((i) => i.args),
  cmd,
);

test('Export VEGAS Relink List saves each moved video\'s old path and new path', async ({ page }) => {
  await loadHarness(page);
  await page.locator('#vegas-relink-export-btn').click();
  await expect(page.locator('#toast-container')).toContainText('Relink list saved (1 moved video)');
  const [dialog] = await calls(page, 'plugin:dialog|save');
  expect(dialog.options.defaultPath).toBe('H:\\Movie\\Armory\\dod_highlights.relink.json');
  const writes = await calls(page, 'save_project_session');
  expect(writes).toHaveLength(1);
  expect(writes[0].path).toBe(SAVE_TO);
  expect(JSON.parse(writes[0].contents)).toEqual({
    format: 'dodstudio-relink-map',
    version: 1,
    moves: [{ from: 'E:\\Unsorted\\dyelife_row11.mov', to: 'H:\\Movie\\Armory\\dyelife.mov', why: 'moved' }],
  });
  await expect(page.locator('#vegas-relink-export-btn')).toBeEnabled();
  await expect(page.locator('#vegas-relink-progress')).toBeHidden();
});

test('From a Project Backup adds the backup\'s paths and never writes the backup', async ({ page }) => {
  const backupText = JSON.stringify({
    version: '0.12.0',
    takeIndex: {},
    takeRenders: {
      'session_a/chain_00_b0': [{ started_unix_ms: 1000, stream: 'all', output_path: 'E:\\Unsorted\\dyelife_row11.mov' }],
      // Moved before Studio kept old paths: only the backup knows this one.
      'session_a/chain_01_b0': [{ started_unix_ms: 2000, stream: 'all', output_path: 'D:\\Old\\gskill.mov' }],
      'session_a/deleted_take': [{ started_unix_ms: 3000, stream: 'all', output_path: 'D:\\Old\\gone.mov' }],
    },
  });
  await loadHarness(page, { backupText });
  await page.locator('#vegas-relink-backup-btn').click();
  await expect(page.locator('#toast-container')).toContainText('Relink list saved (2 moved videos)');
  expect((await calls(page, 'load_project_session')).map((a) => a.path)).toEqual([BACKUP]);
  const writes = await calls(page, 'save_project_session');
  expect(writes.map((w) => w.path)).toEqual([SAVE_TO]);
  const { moves } = JSON.parse(writes[0].contents);
  expect(moves).toContainEqual({ from: 'D:\\Old\\gskill.mov', to: 'E:\\Unsorted\\gskill_row12.mov', why: 'moved' });
  expect(moves).toHaveLength(2);
});

test('a cancelled save writes nothing and puts the buttons back', async ({ page }) => {
  await loadHarness(page, { save: null });
  await page.locator('#vegas-relink-export-btn').click();
  await expect(page.locator('#vegas-relink-export-btn')).toBeEnabled();
  expect(await calls(page, 'plugin:dialog|save')).toHaveLength(1);
  expect(await calls(page, 'save_project_session')).toEqual([]);
});
