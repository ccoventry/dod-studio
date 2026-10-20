// auditor_pane.js + audit_folders.js — Demo Auditor › Duplicates (#844).
//
// Pins the Folders to check list (the Target Folder to start, picked folders, All
// drives), what Start Audit sends, the progress bar and step line while it
// runs, the kept copy and the in-use copies left unticked, and the two
// deletes: the Recycle Bin by default, permanent as its own choice.
import { test, expect } from '@playwright/test';

const GROUPS = [
  {
    size: 2 * 1024 * 1024,
    header_hash: 0xabc,
    files: [
      { path: '\\\\?\\C:\\Half-Life\\dod\\match.dem', in_use: 'game_folder' },
      { path: '\\\\?\\F:\\downloads\\match.dem', in_use: null },
      { path: '\\\\?\\G:\\project\\match.dem', in_use: 'queue' },
    ],
  },
  {
    size: 1024 * 1024,
    header_hash: 0xdef,
    files: [
      { path: '\\\\?\\D:\\a.dem', in_use: null },
      { path: '\\\\?\\E:\\old\\a.dem', in_use: null },
    ],
  },
];

async function loadHarness(page, { picked = null, drives = ['C:\\', 'F:\\'] } = {}) {
  await page.addInitScript(({ picked, drives, groups }) => {
    window.__mockInvokeHandlers = window.__mockInvokeHandlers || {};
    window.__mockInvokeHandlers['plugin:dialog|open'] = () => picked;
    window.__mockInvokeHandlers.list_fixed_drives = () => drives;
    window.__mockInvokeHandlers.run_demo_audit = () => new Promise((resolve) => {
      window.__finishAudit = (outcome) => resolve(outcome ?? { groups, demos: 5, cancelled: false });
    });
    window.__mockInvokeHandlers.delete_audit_files = ({ paths }) => ({ deleted: paths, failed: [] });
  }, { picked, drives, groups: GROUPS });
  await page.goto('/tests/e2e/duplicates.html');
  await page.waitForFunction(() => window.__harnessReady === true);
}

const calls = (page, cmd) => page.evaluate((c) => window.__mockInvocations.filter((i) => i.cmd === c).map((i) => i.args), cmd);
const confirm = (page) => page.click('#themed-confirm-ok-btn');

async function runAudit(page) {
  await page.fill('#audit-target-folder-input', 'C:\\Demos');
  await page.click('#run-audit-btn');
  await page.evaluate(() => window.__finishAudit());
  await expect(page.locator('#audit-status-text')).toHaveText('Done: 5 demos checked, 2 with copies.');
}

test('the folder list holds the Target Folder and picked folders, once each', async ({ page }) => {
  await loadHarness(page, { picked: ['F:\\Downloads', 'c:\\demos\\', 'G:\\'] });
  await expect(page.locator('#run-audit-btn')).toBeDisabled();
  await expect(page.locator('#audit-all-drives-label')).toHaveText('All drives (C:, F:)');

  await page.fill('#audit-target-folder-input', 'C:\\Demos');
  await page.click('#audit-add-folder-btn');
  await expect(page.locator('#audit-folder-list .audit-folder-path')).toHaveText(['C:\\Demos', 'F:\\Downloads', 'G:\\']);
  // Every folder can be removed, the Target Folder's too.
  await expect(page.locator('#audit-folder-list li button')).toHaveCount(3);
  await expect(page.locator('#run-audit-btn')).toBeEnabled();

  await page.locator('#audit-folder-list li', { hasText: 'G:\\' }).locator('button').click();
  await expect(page.locator('#audit-folder-list .audit-folder-path')).toHaveText(['C:\\Demos', 'F:\\Downloads']);
  const picker = await calls(page, 'plugin:dialog|open');
  expect(picker[0].options.directory).toBe(true);
  expect(picker[0].options.multiple).toBe(true);
});

test('the list follows the Target Folder until it is edited here', async ({ page }) => {
  await loadHarness(page, { picked: ['F:\\Downloads'] });
  await page.fill('#audit-target-folder-input', 'C:\\Demos');
  await page.fill('#audit-target-folder-input', 'D:\\Other');
  await expect(page.locator('#audit-folder-list .audit-folder-path')).toHaveText(['D:\\Other']);
  await page.click('#audit-add-folder-btn');
  // Edited: the other tabs' folder no longer changes this list.
  await page.fill('#audit-target-folder-input', 'E:\\Elsewhere');
  await expect(page.locator('#audit-folder-list .audit-folder-path')).toHaveText(['D:\\Other', 'F:\\Downloads']);
  // Emptied: it follows the Target Folder again.
  await page.locator('#audit-folder-list li button').first().click();
  await page.locator('#audit-folder-list li button').first().click();
  await page.fill('#audit-target-folder-input', 'G:\\Again');
  await expect(page.locator('#audit-folder-list .audit-folder-path')).toHaveText(['G:\\Again']);
});

test('All drives alone can start an audit and dims the list', async ({ page }) => {
  await loadHarness(page);
  await page.check('#audit-all-drives');
  await expect(page.locator('#audit-folder-list')).toHaveClass(/dimmed/);
  await expect(page.locator('#audit-add-folder-btn')).toBeDisabled();
  await expect(page.locator('#run-audit-btn')).toBeEnabled();
  await page.click('#run-audit-btn');
  const [args] = await calls(page, 'run_demo_audit');
  expect(args).toEqual({ paths: [], allDrives: true, queued: [] });
});

test('a running audit shows its step and a bar, and locks its controls', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => { window.__queue = [{ path: 'G:\\project\\match.dem' }, {}]; });
  await page.fill('#audit-target-folder-input', 'C:\\Demos');
  await page.click('#run-audit-btn');

  const [args] = await calls(page, 'run_demo_audit');
  expect(args).toEqual({ paths: ['C:\\Demos'], allDrives: false, queued: ['G:\\project\\match.dem'] });
  await expect(page.locator('#run-audit-btn')).toBeDisabled();
  await expect(page.locator('#cancel-audit-btn')).toBeEnabled();
  await expect(page.locator('#audit-all-drives')).toBeDisabled();
  // The harness has no CSS, so the empty bar has no size to be "visible".
  await expect(page.locator('#audit-progress')).not.toHaveAttribute('hidden');

  await page.evaluate(() => window.__mockEmit('audit_progress', { folders: 12, total: 0, done: 0, current: '\\\\?\\C:\\Demos\\x' }));
  await expect(page.locator('#audit-status-text')).toHaveText('Looking for demos: 12 folders read');
  await expect(page.locator('#audit-status-current')).toHaveText('C:\\Demos\\x');
  await page.evaluate(() => window.__mockEmit('audit_progress', { folders: 12, total: 4, done: 1, current: 'C:\\Demos\\a.dem' }));
  await expect(page.locator('#audit-status-text')).toHaveText('Comparing demos: 1 / 4');
  await expect(page.locator('#audit-progress .progress-bar-fill')).toHaveAttribute('style', /width: 25%/);

  await page.click('#cancel-audit-btn');
  expect(await calls(page, 'cancel_audit')).toHaveLength(1);
  await page.evaluate(() => window.__finishAudit({ groups: [], demos: 1, cancelled: true }));
  await expect(page.locator('#audit-status-text')).toHaveText('Cancelled. Nothing was listed.');
  await expect(page.locator('#audit-results-body')).toContainText('cancelled before it finished');
  await expect(page.locator('#audit-progress')).toHaveAttribute('hidden');
  await expect(page.locator('#run-audit-btn')).toBeEnabled();
});

test('the kept copy comes first and copies in use start unticked', async ({ page }) => {
  await loadHarness(page);
  await runAudit(page);

  const rows = page.locator('#audit-results-body tr.audit-file-row');
  await expect(rows.locator('.audit-file-path')).toHaveText([
    'C:\\Half-Life\\dod\\match.dem', 'F:\\downloads\\match.dem', 'G:\\project\\match.dem',
    'D:\\a.dem', 'E:\\old\\a.dem',
  ]);
  await expect(rows.locator('.audit-file-status')).toHaveText([
    'Kept (in the game folder)', '↳ Copy', "↳ Copy, in this project's queue", 'Kept', '↳ Copy',
  ]);
  const boxes = rows.locator('input.audit-row-cb');
  await expect(boxes.nth(0)).toBeDisabled();
  await expect(boxes.nth(1)).toBeChecked();
  await expect(boxes.nth(2)).not.toBeChecked();
  await expect(boxes.nth(4)).toBeChecked();
  await expect(page.locator('#delete-selected-audits-btn')).toHaveText('Move 2 to Recycle Bin');
  await expect(page.locator('#delete-permanent-audits-btn')).toHaveText('Delete 2 Permanently…');
  await expect(page.locator('#audit-list-footer')).toHaveText('Duplicates Found: 3 | Wasted Space: 5.0 MB');
});

test('another copy can be kept: it moves to the top, the old pick becomes a ticked copy', async ({ page }) => {
  await loadHarness(page);
  await runAudit(page);
  const rows = page.locator('#audit-results-body tr.audit-file-row');
  // Keep E:\old\a.dem instead of D:\a.dem.
  // A click, not check(): the list redraws at once, so check() can't see it stay ticked.
  await rows.nth(4).locator('input[type=radio]').click();
  await expect(rows.locator('.audit-file-path')).toHaveText([
    'C:\\Half-Life\\dod\\match.dem', 'F:\\downloads\\match.dem', 'G:\\project\\match.dem',
    'E:\\old\\a.dem', 'D:\\a.dem',
  ]);
  await expect(rows.nth(3).locator('.audit-file-status')).toHaveText('Kept');
  await expect(rows.nth(3).locator('input.audit-row-cb')).toBeDisabled();
  await expect(rows.nth(4).locator('input.audit-row-cb')).toBeChecked();

  await page.click('#delete-selected-audits-btn');
  await confirm(page);
  const [args] = await calls(page, 'delete_audit_files');
  // The kept copy is never sent.
  expect(args.paths).toEqual(['\\\\?\\F:\\downloads\\match.dem', '\\\\?\\D:\\a.dem']);
});

test('Keep copies already in a folder picks the copy in that folder in every group', async ({ page }) => {
  await loadHarness(page, { picked: 'E:\\old' });
  await expect(page.locator('#audit-prefer-folder-btn')).toBeDisabled();
  await runAudit(page);
  await page.click('#audit-prefer-folder-btn');
  await expect(page.locator('.toast').last()).toContainText('in 1 group (1 changed)');
  const rows = page.locator('#audit-results-body tr.audit-file-row');
  await expect(rows.nth(3).locator('.audit-file-path')).toHaveText('E:\\old\\a.dem');
  await expect(rows.nth(3).locator('.audit-file-status')).toHaveText('Kept');
});

test('Move to Recycle Bin is the default delete, and the list follows it', async ({ page }) => {
  await loadHarness(page);
  await runAudit(page);
  await page.click('#delete-selected-audits-btn');
  await expect(page.locator('#themed-confirm-message')).toContainText('Recycle Bin');
  await confirm(page);

  const [args] = await calls(page, 'delete_audit_files');
  expect(args).toEqual({
    paths: ['\\\\?\\F:\\downloads\\match.dem', '\\\\?\\E:\\old\\a.dem'],
    permanent: false,
  });
  await expect(page.locator('#audit-status-text')).toHaveText('Moved to the Recycle Bin: 2.');
  // The D:/E: group has no copies left; the queue copy stays listed.
  await expect(page.locator('#audit-results-body .audit-file-path')).toHaveText([
    'C:\\Half-Life\\dod\\match.dem', 'G:\\project\\match.dem',
  ]);
  await expect(page.locator('#delete-selected-audits-btn')).toBeDisabled();
});

test('Delete Permanently asks on its own and sends permanent', async ({ page }) => {
  await loadHarness(page);
  await runAudit(page);
  await page.click('#delete-permanent-audits-btn');
  await expect(page.locator('#themed-confirm-message')).toContainText("can't be put back");
  await page.click('#themed-confirm-cancel-btn');
  expect(await calls(page, 'delete_audit_files')).toHaveLength(0);

  await page.click('#delete-permanent-audits-btn');
  await confirm(page);
  const [args] = await calls(page, 'delete_audit_files');
  expect(args.permanent).toBe(true);
  await expect(page.locator('#audit-status-text')).toHaveText('Deleted: 2.');
});

// Consolidate… (#872): audit_consolidate_ui.js.
const PLAN = {
  destination: 'H:\\All',
  moves: [
    { from: '\\\\?\\D:\\a.dem', to: 'H:\\All\\a.dem', size: 1048576, cross_drive: true, renamed: false },
    { from: '\\\\?\\G:\\solo.dem', to: 'H:\\All\\solo.dem', size: 1048576, cross_drive: true, renamed: false },
  ],
  recycle: ['\\\\?\\E:\\old\\a.dem', '\\\\?\\F:\\downloads\\match.dem', '\\\\?\\G:\\project\\match.dem'],
  stay_in_game_folders: 1,
  copies_in_game_folders: 0,
  already_there: 0,
  renamed: 0,
  move_bytes: 2097152,
  copy_bytes: 2097152,
  free_bytes: 1e12,
  enough_space: true,
};

async function openConsolidate(page, { plan = PLAN } = {}) {
  await page.evaluate((plan) => {
    window.__mockInvokeHandlers.consolidate_plan = () => plan;
    window.__mockInvokeHandlers.consolidate_undoable = () => window.__undoable ?? null;
  }, plan);
  await page.fill('#audit-target-folder-input', 'C:\\Demos');
  await page.click('#run-audit-btn');
  await page.evaluate((groups) => window.__finishAudit({
    groups,
    singles: [{ path: '\\\\?\\G:\\solo.dem', size: 1048576, in_use: null }],
    roots: ['C:\\Demos'],
    demos: 6,
    cancelled: false,
  }), GROUPS);
  await page.click('#audit-consolidate-btn');
}

test('Consolidate previews the plan, then moves, updates the queue and offers Undo', async ({ page }) => {
  await loadHarness(page, { picked: 'H:\\All' });
  await openConsolidate(page);
  await expect(page.locator('#audit-consolidate-preview')).toHaveText('Choose the folder to move the demos into.');
  await expect(page.locator('#audit-consolidate-start')).toBeDisabled();

  await page.check('input[name="audit-consolidate-layout"][value="subfolders"]');
  await page.click('#audit-consolidate-browse');
  await expect(page.locator('#audit-consolidate-dest')).toHaveValue('H:\\All');
  await expect(page.locator('#audit-consolidate-preview')).toHaveText(
    'Move 2 demos (2.0 MB) into H:\\All, recycle 3 copies. 1 in game folders left where they are.',
  );
  const plans = await calls(page, 'consolidate_plan');
  const req = plans.at(-1).request;
  expect(req).toMatchObject({ destination: 'H:\\All', layout: 'subfolders', include_game_folders: false, roots: ['C:\\Demos'] });
  // Each group's kept copy and the demo without copies move; the rest recycle.
  expect(req.keep.map((d) => d.path)).toEqual(['\\\\?\\C:\\Half-Life\\dod\\match.dem', '\\\\?\\D:\\a.dem', '\\\\?\\G:\\solo.dem']);
  expect(req.keep[0].game_folder).toBe(true);
  expect(req.recycle.map((d) => d.path)).toEqual([
    '\\\\?\\F:\\downloads\\match.dem', '\\\\?\\G:\\project\\match.dem', '\\\\?\\E:\\old\\a.dem',
  ]);

  // Nothing moves until it's confirmed.
  await page.evaluate(() => {
    window.__mockInvokeHandlers.consolidate_run = () => new Promise((resolve) => { window.__finishRun = resolve; });
  });
  await page.click('#audit-consolidate-start');
  await page.click('#themed-confirm-cancel-btn');
  expect(await calls(page, 'consolidate_run')).toHaveLength(0);

  await page.click('#audit-consolidate-start');
  await confirm(page);
  await expect(page.locator('#audit-consolidate-start')).toBeDisabled();
  await expect(page.locator('#audit-consolidate-cancel')).toBeEnabled();
  await expect(page.locator('#run-audit-btn')).toBeDisabled();
  await page.evaluate(() => window.__mockEmit('consolidate_progress', {
    step: 'moving', files_done: 1, files_total: 2, bytes_done: 1048576, bytes_total: 2097152, current: 'D:\\a.dem',
  }));
  await expect(page.locator('#audit-status-text')).toHaveText('Moving demos: 1 / 2 (1.0 MB of 2.0 MB)');
  await expect(page.locator('#audit-progress .progress-bar-fill')).toHaveAttribute('style', /width: 50%/);

  await page.evaluate(() => {
    window.__undoable = { log: 'x.json', count: 2, destination: 'H:\\All', created_unix_secs: 1 };
    window.__finishRun({
      moved: [{ from: 'D:\\a.dem', to: 'H:\\All\\a.dem' }, { from: 'G:\\solo.dem', to: 'H:\\All\\solo.dem' }],
      failed: [],
      recycled: ['\\\\?\\E:\\old\\a.dem'],
      recycle_failed: [],
      cancelled: false,
      log: 'x.json',
    });
  });
  await expect(page.locator('#audit-status-text')).toHaveText("Moved 2 demos, recycled 1 copy. 2 in this project's queue updated.");
  expect(await page.evaluate(() => window.__relocated)).toEqual([
    { from: 'D:\\a.dem', to: 'H:\\All\\a.dem' }, { from: 'G:\\solo.dem', to: 'H:\\All\\solo.dem' },
  ]);
  await expect(page.locator('#audit-results-body')).toContainText('Run the audit again');
  await expect(page.locator('#audit-progress')).toHaveAttribute('hidden');
  await expect(page.locator('#audit-consolidate-undo')).toBeEnabled();
  await expect(page.locator('#run-audit-btn')).toBeEnabled();

  // Undo puts them back, and the queue follows again.
  await page.evaluate(() => {
    window.__mockInvokeHandlers.consolidate_undo = () => {
      window.__undoable = null;
      return { moved: [{ from: 'H:\\All\\a.dem', to: 'D:\\a.dem' }], failed: [], recycled: [], recycle_failed: [], cancelled: false, log: 'x.json' };
    };
  });
  await page.click('#audit-consolidate-undo');
  await confirm(page);
  await expect(page.locator('#audit-status-text')).toHaveText("Put back 1 demo. 1 in this project's queue updated.");
  await expect(page.locator('#audit-consolidate-undo')).toBeDisabled();
});

test('Consolidate can be cancelled while it moves', async ({ page }) => {
  await loadHarness(page, { picked: 'H:\\All' });
  await openConsolidate(page);
  await page.click('#audit-consolidate-browse');
  await page.evaluate(() => {
    window.__mockInvokeHandlers.consolidate_run = () => new Promise((resolve) => { window.__finishRun = resolve; });
  });
  await page.click('#audit-consolidate-start');
  await confirm(page);
  await page.click('#audit-consolidate-cancel');
  expect(await calls(page, 'consolidate_cancel')).toHaveLength(1);
  await page.evaluate(() => window.__finishRun({
    moved: [{ from: 'D:\\a.dem', to: 'H:\\All\\a.dem' }], failed: [], recycled: [], recycle_failed: [], cancelled: true, log: 'x.json',
  }));
  await expect(page.locator('#audit-status-text')).toHaveText(
    "Moved 1 demo. Cancelled: the rest stayed where they were, and no copies were recycled. 1 in this project's queue updated.",
  );
});

test('Consolidate does not start when the copies do not fit', async ({ page }) => {
  await loadHarness(page, { picked: 'H:\\All' });
  await openConsolidate(page, { plan: { ...PLAN, enough_space: false, free_bytes: 1048576 } });
  await page.click('#audit-consolidate-browse');
  await expect(page.locator('#audit-consolidate-preview')).toHaveText(
    'Not enough space: the demos copied from other drives need 2.0 MB and 1.0 MB is free.',
  );
  await expect(page.locator('#audit-consolidate-start')).toBeDisabled();
});

test('a file that fails to delete stays listed and is reported', async ({ page }) => {
  await loadHarness(page);
  await runAudit(page);
  await page.evaluate(() => {
    window.__mockInvokeHandlers.delete_audit_files = ({ paths }) => ({
      deleted: [paths[0]],
      failed: [{ path: paths[1], error: 'Failed to delete E:\\old\\a.dem: in use' }],
    });
  });
  await page.click('#delete-selected-audits-btn');
  await confirm(page);
  await expect(page.locator('#audit-status-text')).toHaveText('Moved to the Recycle Bin: 1, 1 not deleted.');
  await expect(page.locator('#audit-results-body .audit-file-path')).toContainText(['E:\\old\\a.dem']);
});
