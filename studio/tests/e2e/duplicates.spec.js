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
    window.__mockInvokeHandlers.delete_audit_files = ({ copies }) => ({ deleted: copies.map((c) => c.path), failed: [] });
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
  await expect(page.locator('#audit-status-text')).toHaveText(/^Done in \d+\.\d s: 5 demos checked, 2 with copies\.$/);
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
  expect(args).toEqual({ paths: [], allDrives: true, queued: [], lookInZips: false });
});

test('a running audit shows its step and a bar, and locks its controls', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => { window.__queue = [{ path: 'G:\\project\\match.dem' }, {}]; });
  await page.fill('#audit-target-folder-input', 'C:\\Demos');
  await page.click('#run-audit-btn');

  const [args] = await calls(page, 'run_demo_audit');
  expect(args).toEqual({ paths: ['C:\\Demos'], allDrives: false, queued: ['G:\\project\\match.dem'], lookInZips: false });
  await expect(page.locator('#run-audit-btn')).toBeDisabled();
  await expect(page.locator('#cancel-audit-btn')).toBeEnabled();
  await expect(page.locator('#audit-all-drives')).toBeDisabled();
  // The harness has no CSS, so the empty bar has no size to be "visible".
  await expect(page.locator('#audit-progress')).not.toHaveAttribute('hidden');

  // The first audit of these folders has no count to measure the walk by:
  // the bar moves without a width. A clock follows the step line.
  await page.evaluate(() => window.__mockEmit('audit_progress', { folders: 12, total: 0, done: 0, current: '\\\\?\\C:\\Demos\\x' }));
  await expect(page.locator('#audit-status-text')).toHaveText(/^Looking for demos: 12 folders read · 0:0\d$/);
  await expect(page.locator('#audit-status-current')).toHaveText('C:\\Demos\\x');
  await expect(page.locator('#audit-progress')).toHaveClass(/indeterminate/);
  await page.evaluate(() => window.__mockEmit('audit_progress', { folders: 12, total: 4, done: 1, current: 'C:\\Demos\\a.dem' }));
  await expect(page.locator('#audit-status-text')).toHaveText(/^Comparing demos: 1 \/ 4 · 0:0\d$/);
  await expect(page.locator('#audit-progress .progress-bar-fill')).toHaveAttribute('style', /width: 25%/);
  await expect(page.locator('#audit-progress')).not.toHaveClass(/indeterminate/);

  await page.click('#cancel-audit-btn');
  expect(await calls(page, 'cancel_audit')).toHaveLength(1);
  await page.evaluate(() => window.__finishAudit({ groups: [], demos: 1, cancelled: true }));
  await expect(page.locator('#audit-status-text')).toHaveText('Cancelled. Nothing was listed.');
  await expect(page.locator('#audit-results-body')).toContainText('cancelled before it finished');
  await expect(page.locator('#audit-progress')).toHaveAttribute('hidden');
  await expect(page.locator('#run-audit-btn')).toBeEnabled();
});

test('the next audit of the same folders measures its walk against the last one', async ({ page }) => {
  await loadHarness(page);
  await page.fill('#audit-target-folder-input', 'C:\\Demos');
  await page.click('#run-audit-btn');
  await page.evaluate(() => window.__mockEmit('audit_progress', { folders: 40, total: 0, done: 0 }));
  await page.evaluate(() => window.__finishAudit());
  await expect(page.locator('#audit-status-text')).toHaveText(/^Done in /);

  await page.click('#run-audit-btn');
  await page.evaluate(() => window.__mockEmit('audit_progress', { folders: 10, total: 0, done: 0 }));
  await expect(page.locator('#audit-status-text')).toHaveText(/^Looking for demos: 10 folders read \(about 40 last time\) · /);
  await expect(page.locator('#audit-progress .progress-bar-fill')).toHaveAttribute('style', /width: 25%/);
  await expect(page.locator('#audit-progress')).not.toHaveClass(/indeterminate/);
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

test('another copy can be kept: rows stay put, the old pick becomes a ticked copy, Reset undoes it', async ({ page }) => {
  await loadHarness(page);
  await runAudit(page);
  const rows = page.locator('#audit-results-body tr.audit-file-row');
  const order = [
    'C:\\Half-Life\\dod\\match.dem', 'F:\\downloads\\match.dem', 'G:\\project\\match.dem',
    'D:\\a.dem', 'E:\\old\\a.dem',
  ];
  await expect(page.locator('.audit-keep-changed')).toHaveCount(0);
  // Keep E:\old\a.dem instead of D:\a.dem.
  // A click, not check(): the list redraws at once, so check() can't see it stay ticked.
  await rows.nth(4).locator('input[type=radio]').click();
  await expect(rows.locator('.audit-file-path')).toHaveText(order);
  await expect(rows.nth(4).locator('.audit-file-status')).toHaveText('Kept');
  await expect(rows.nth(4)).toHaveClass(/audit-kept/);
  await expect(rows.nth(4).locator('input.audit-row-cb')).toBeDisabled();
  await expect(rows.nth(3).locator('.audit-file-status')).toHaveText('↳ Copy');
  await expect(rows.nth(3).locator('input.audit-row-cb')).toBeChecked();
  // Only that group says its kept copy was changed.
  await expect(page.locator('.audit-keep-changed')).toHaveCount(1);
  await expect(page.locator('.audit-keep-changed')).toContainText('Kept copy changed by you');

  await page.click('#delete-selected-audits-btn');
  await confirm(page);
  const [args] = await calls(page, 'delete_audit_files');
  // The kept copy is never sent to be deleted, only as what each copy must match.
  expect(args.copies).toEqual([
    { path: '\\\\?\\F:\\downloads\\match.dem', keep: '\\\\?\\C:\\Half-Life\\dod\\match.dem' },
    { path: '\\\\?\\D:\\a.dem', keep: '\\\\?\\E:\\old\\a.dem' },
  ]);
});

test('Reset keeps the audit\'s pick again', async ({ page }) => {
  await loadHarness(page);
  await runAudit(page);
  const rows = page.locator('#audit-results-body tr.audit-file-row');
  await rows.nth(4).locator('input[type=radio]').click();
  await page.click('.audit-keep-reset-btn');
  await expect(page.locator('.audit-keep-changed')).toHaveCount(0);
  await expect(rows.nth(3).locator('.audit-file-status')).toHaveText('Kept');
  await expect(rows.nth(4).locator('input.audit-row-cb')).toBeChecked();
  await expect(page.locator('#delete-selected-audits-btn')).toHaveText('Move 2 to Recycle Bin');
});

test('Keep copies already in a folder picks the copy in that folder in every group', async ({ page }) => {
  await loadHarness(page, { picked: 'E:\\old' });
  await expect(page.locator('#audit-prefer-folder-btn')).toBeDisabled();
  await runAudit(page);
  await page.click('#audit-prefer-folder-btn');
  await expect(page.locator('.toast').last()).toContainText('in 1 group (1 changed)');
  const rows = page.locator('#audit-results-body tr.audit-file-row');
  await expect(rows.nth(4).locator('.audit-file-path')).toHaveText('E:\\old\\a.dem');
  await expect(rows.nth(4).locator('.audit-file-status')).toHaveText('Kept');
  await expect(page.locator('.audit-keep-changed')).toHaveCount(1);
});

test('Move to Recycle Bin is the default delete, and the list follows it', async ({ page }) => {
  await loadHarness(page);
  await runAudit(page);
  await page.click('#delete-selected-audits-btn');
  await expect(page.locator('#themed-confirm-message')).toContainText('Recycle Bin');
  await confirm(page);

  const [args] = await calls(page, 'delete_audit_files');
  expect(args).toEqual({
    copies: [
      { path: '\\\\?\\F:\\downloads\\match.dem', keep: '\\\\?\\C:\\Half-Life\\dod\\match.dem' },
      { path: '\\\\?\\E:\\old\\a.dem', keep: '\\\\?\\D:\\a.dem' },
    ],
    permanent: false,
  });
  await expect(page.locator('#audit-status-text')).toHaveText('Moved to the Recycle Bin: 2.');
  // The D:/E: group has no copies left; the queue copy stays listed.
  await expect(page.locator('#audit-results-body .audit-file-path')).toHaveText([
    'C:\\Half-Life\\dod\\match.dem', 'G:\\project\\match.dem',
  ]);
  await expect(page.locator('#delete-selected-audits-btn')).toBeDisabled();
});

test('the delete line says copies are being checked before any is deleted', async ({ page }) => {
  await loadHarness(page);
  await page.evaluate(() => {
    window.__mockInvokeHandlers.delete_audit_files = () => new Promise((resolve) => { window.__finishDelete = resolve; });
  });
  await runAudit(page);
  await page.click('#delete-selected-audits-btn');
  await confirm(page);
  await expect(page.locator('#audit-status-text')).toHaveText('Checking each copy matches the kept one byte for byte: 0 / 2');
  await page.evaluate(() => window.__mockEmit('audit_delete_progress', { step: 'checking', done: 1, total: 2 }));
  await expect(page.locator('#audit-status-text')).toHaveText('Checking each copy matches the kept one byte for byte: 1 / 2');
  await page.evaluate(() => window.__mockEmit('audit_delete_progress', { step: 'deleting', done: 1, total: 2 }));
  await expect(page.locator('#audit-status-text')).toHaveText('Moving to the Recycle Bin: 1 / 2');
  await page.evaluate(() => window.__finishDelete({ deleted: [], failed: [] }));
});

test('Delete Permanently is hidden for 0.2.0', async ({ page }) => {
  await loadHarness(page);
  await runAudit(page);
  await expect(page.locator('#delete-permanent-audits-btn')).toBeHidden();
});

test('Delete Permanently, when offered, asks on its own and sends permanent', async ({ page }) => {
  await loadHarness(page);
  await runAudit(page);
  await page.evaluate(() => document.querySelector('#delete-permanent-audits-btn').removeAttribute('hidden'));
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
    'Move 2 demos (2.0 MB) into H:\\All, recycle 3 copies (each first checked byte for byte against the demo kept). 1 in game folders left where they are.',
  );
  const plans = await calls(page, 'consolidate_plan');
  const req = plans.at(-1).request;
  expect(req).toMatchObject({ destination: 'H:\\All', layout: 'subfolders', include_game_folders: false, roots: ['C:\\Demos'] });
  // Each group's kept copy and the demo without copies move; the rest
  // recycle, each once it matches its group's kept copy.
  expect(req.keep.map((d) => d.path)).toEqual(['\\\\?\\C:\\Half-Life\\dod\\match.dem', '\\\\?\\D:\\a.dem', '\\\\?\\G:\\solo.dem']);
  expect(req.keep[0].game_folder).toBe(true);
  expect(req.recycle.map((d) => [d.path, d.keep])).toEqual([
    ['\\\\?\\F:\\downloads\\match.dem', '\\\\?\\C:\\Half-Life\\dod\\match.dem'],
    ['\\\\?\\G:\\project\\match.dem', '\\\\?\\C:\\Half-Life\\dod\\match.dem'],
    ['\\\\?\\E:\\old\\a.dem', '\\\\?\\D:\\a.dem'],
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
  // The copies are compared before any is recycled.
  await page.evaluate(() => window.__mockEmit('consolidate_progress', {
    step: 'checking', files_done: 1, files_total: 3, bytes_done: 0, bytes_total: 0, current: 'E:\\old\\a.dem',
  }));
  await expect(page.locator('#audit-status-text')).toHaveText('Checking each copy matches the kept one byte for byte: 1 / 3');

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
  // The harness counts every pair it's handed as a queue row.
  await expect(page.locator('#audit-status-text')).toHaveText("Moved 2 demos, recycled 1 copy. 3 in this project's queue updated.");
  expect(await page.evaluate(() => window.__relocated)).toEqual([
    { from: 'D:\\a.dem', to: 'H:\\All\\a.dem' }, { from: 'G:\\solo.dem', to: 'H:\\All\\solo.dem' },
    // A queued row on the recycled copy follows its kept demo to where it moved.
    { from: 'E:\\old\\a.dem', to: 'H:\\All\\a.dem' },
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

test('a copy the delete finds different is marked, unticked and counted out', async ({ page }) => {
  await loadHarness(page);
  await runAudit(page);
  await page.evaluate(() => {
    window.__mockInvokeHandlers.delete_audit_files = ({ copies }) => ({
      deleted: [copies[0].path],
      failed: [{ path: copies[1].path, error: 'Not deleted: E:\\old\\a.dem is not an exact copy of D:\\a.dem', not_a_copy: true }],
    });
  });
  await page.click('#delete-selected-audits-btn');
  await confirm(page);
  const row = page.locator('#audit-results-body tr.audit-file-row', { hasText: 'E:\\old\\a.dem' });
  await expect(row.locator('.audit-file-status')).toHaveText('≠ Not an exact copy');
  await expect(row.locator('input.audit-row-cb')).toBeDisabled();
  await expect(row.locator('input.audit-row-cb')).not.toBeChecked();
  // Select all skips it; the footer no longer counts it as wasted space.
  await page.check('#audit-select-all');
  await expect(row.locator('input.audit-row-cb')).not.toBeChecked();
  await expect(page.locator('#audit-list-footer')).toHaveText('Duplicates Found: 1 | Wasted Space: 2.0 MB');
  // Keeping another copy clears the mark: it was measured against the old pick.
  await row.locator('input[type=radio]').click();
  await expect(page.locator('#audit-results-body')).not.toContainText('Not an exact copy');
});

test('Consolidate never sends a copy marked different, and marks one it finds', async ({ page }) => {
  await loadHarness(page, { picked: 'H:\\All' });
  await openConsolidate(page);
  // The delete finds E:\old\a.dem differs from D:\a.dem.
  await page.evaluate(() => {
    window.__mockInvokeHandlers.delete_audit_files = ({ copies }) => ({
      deleted: [],
      failed: copies.filter((c) => c.path.includes('old')).map((c) => ({ path: c.path, error: 'differs', not_a_copy: true })),
    });
  });
  await page.click('#delete-selected-audits-btn');
  await confirm(page);
  await page.click('#audit-consolidate-browse');
  await expect(page.locator('#audit-consolidate-start')).toBeEnabled();
  const req = (await calls(page, 'consolidate_plan')).at(-1).request;
  expect(req.recycle.map((d) => d.path)).toEqual(['\\\\?\\F:\\downloads\\match.dem', '\\\\?\\G:\\project\\match.dem']);

  // The run's own check finds F:\downloads\match.dem differs: kept, said, marked.
  await page.evaluate(() => {
    window.__mockInvokeHandlers.consolidate_run = () => ({
      moved: [],
      failed: [],
      recycled: [],
      recycle_failed: [{
        path: '\\\\?\\F:\\downloads\\match.dem',
        error: 'Not deleted: F:\\downloads\\match.dem is not an exact copy of C:\\Half-Life\\dod\\match.dem (they differ from byte 9), so it may be a different demo',
        not_a_copy: true,
      }],
      cancelled: false,
      log: null,
    });
  });
  await page.click('#audit-consolidate-start');
  await confirm(page);
  await expect(page.locator('#audit-status-text')).toHaveText('Moved 0 demos. 1 copy was kept: not an exact copy of the demo kept.');
  await expect(page.locator('#toast-container')).toContainText('is not an exact copy of C:\\Half-Life\\dod\\match.dem');
  const row = page.locator('#audit-results-body tr.audit-file-row', { hasText: 'F:\\downloads\\match.dem' });
  await expect(row.locator('.audit-file-status')).toHaveText('≠ Not an exact copy');
  await expect(row.locator('input.audit-row-cb')).toBeDisabled();
});

test('a file that fails to delete stays listed and is reported', async ({ page }) => {
  await loadHarness(page);
  await runAudit(page);
  await page.evaluate(() => {
    window.__mockInvokeHandlers.delete_audit_files = ({ copies }) => ({
      deleted: [copies[0].path],
      failed: [{ path: copies[1].path, error: 'Failed to delete E:\\old\\a.dem: in use' }],
    });
  });
  await page.click('#delete-selected-audits-btn');
  await confirm(page);
  await expect(page.locator('#audit-status-text')).toHaveText('Moved to the Recycle Bin: 1, 1 not deleted.');
  await expect(page.locator('#audit-results-body .audit-file-path')).toContainText(['E:\\old\\a.dem']);
});

// "Show demos backed up in archives" (#929): an archive is a backup. A
// loose demo it holds gets a chip naming it, groups hold loose copies only
// and are ticked as without archives, and a section below the groups lists
// every backed-up demo, one with no loose copy too.
const LAN = { zip: '\\\\?\\D:\\packs\\lan.zip', entry: 'hltv/m.dem' };
const OLD = { zip: '\\\\?\\E:\\old\\2022.7z', entry: 'm.dem' };
const SOLO = { path: '\\\\?\\C:\\demos\\solo.dem', in_use: null, backups: [{ zip: '\\\\?\\D:\\packs\\cup.rar', entry: 'solo.dem' }] };
const BACKED_UP_GROUPS = [
  {
    size: 1024 * 1024,
    header_hash: 0x123,
    files: [
      { path: '\\\\?\\C:\\demos\\m.dem', in_use: null, backups: [LAN] },
      { path: '\\\\?\\F:\\downloads\\m.dem', in_use: null, backups: [LAN, OLD] },
    ],
  },
];
const BACKED_UP = [...BACKED_UP_GROUPS[0].files, SOLO];

test('Show demos backed up in archives is sent, saved, and its archives show in the progress line', async ({ page }) => {
  await loadHarness(page);
  await expect(page.locator('label:has(#audit-look-in-zips)')).toHaveText('Show demos backed up in archives (.zip, .7z, .rar)');
  await expect(page.locator('#audit-look-in-zips')).not.toBeChecked();
  await page.check('#audit-look-in-zips');
  expect(await page.evaluate(() => window.__settingsSaves)).toBe(1);
  await page.fill('#audit-target-folder-input', 'C:\\Demos');
  await page.click('#run-audit-btn');
  const [args] = await calls(page, 'run_demo_audit');
  expect(args.lookInZips).toBe(true);
  await expect(page.locator('#audit-look-in-zips')).toBeDisabled();
  await page.evaluate(() => window.__mockEmit('audit_progress', {
    folders: 12, zips_total: 4, zips_done: 1, total: 0, done: 0, current: '\\\\?\\D:\\packs\\lan.zip',
  }));
  await expect(page.locator('#audit-status-text')).toHaveText(/^Looking inside archives: 1 \/ 4 · 0:0\d$/);
  await expect(page.locator('#audit-status-current')).toHaveText('D:\\packs\\lan.zip');
  await expect(page.locator('#audit-progress .progress-bar-fill')).toHaveAttribute('style', /width: 25%/);
  await page.evaluate(() => window.__finishAudit({ groups: [], backed_up: [], demos: 2, cancelled: false }));
  await expect(page.locator('#audit-look-in-zips')).toBeEnabled();
});

test('a backed-up demo is marked, and its loose copies are offered as without archives', async ({ page }) => {
  await loadHarness(page);
  await page.fill('#audit-target-folder-input', 'C:\\Demos');
  await page.click('#run-audit-btn');
  await page.evaluate(([groups, backedUp]) => window.__finishAudit({ groups, backed_up: backedUp, demos: 3, archive_demos: 4, cancelled: false }),
    [BACKED_UP_GROUPS, BACKED_UP]);
  await expect(page.locator('#audit-status-text')).toHaveText(/^Done in \d+\.\d s: 3 demos checked and 4 inside archives, \d+ with copies\.$/);

  const rows = page.locator('#audit-results-body tr.audit-file-row');
  await expect(rows.locator('.audit-file-path')).toHaveText([
    'C:\\demos\\m.dem backed up in lan.zip',
    'F:\\downloads\\m.dem backed up in 2 archives',
    // The section below the groups: every backed-up demo.
    'C:\\demos\\m.dem backed up in lan.zip',
    'F:\\downloads\\m.dem backed up in 2 archives',
    'C:\\demos\\solo.dem backed up in cup.rar',
  ]);
  const chip = rows.nth(1).locator('.audit-backup-chip');
  await expect(chip).toHaveAttribute('title', /D:\\packs\\lan\.zip › hltv\/m\.dem/);
  await expect(chip).toHaveAttribute('title', /E:\\old\\2022\.7z › m\.dem/);
  // The group works as with no archive: kept first, the copy ticked.
  await expect(rows.nth(0).locator('.audit-file-status')).toHaveText('Kept');
  await expect(rows.nth(1).locator('input.audit-row-cb')).toBeChecked();
  // Nothing in the section can be ticked.
  await expect(rows.nth(4).locator('input')).toHaveCount(0);
  await expect(page.locator('.audit-backed-up-row .group-toggle-btn')).toHaveText('▼ Backed up in archives (3 demos)');
  await expect(page.locator('#audit-list-footer')).toHaveText('Duplicates Found: 1 | Wasted Space: 1.0 MB | Backed up in archives: 3');

  // Open Folder shows the loose demo, not the archive.
  await rows.nth(4).getByText('Open Folder').click();
  const [reveal] = await calls(page, 'reveal_in_explorer');
  expect(reveal.path).toBe('C:\\demos\\solo.dem');

  // The delete is the usual one, checked against the kept loose copy.
  await page.click('#delete-selected-audits-btn');
  await confirm(page);
  const [del] = await calls(page, 'delete_audit_files');
  expect(del.copies).toEqual([{ path: '\\\\?\\F:\\downloads\\m.dem', keep: '\\\\?\\C:\\demos\\m.dem' }]);
  // The deleted copy leaves the section too.
  await expect(page.locator('.audit-backed-up-row .group-toggle-btn')).toHaveText('▼ Backed up in archives (2 demos)');
  await expect(page.locator('#audit-list-footer')).toHaveText('Duplicates Found: 0 | Wasted Space: 0.0 MB | Backed up in archives: 2');
});

test('a backed-up demo with no loose copy shows in the section, which folds', async ({ page }) => {
  await loadHarness(page);
  await page.fill('#audit-target-folder-input', 'C:\\Demos');
  await page.click('#run-audit-btn');
  await page.evaluate((solo) => window.__finishAudit({ groups: [], backed_up: [solo], demos: 1, cancelled: false }), SOLO);
  await expect(page.locator('#audit-results-body .table-empty')).toHaveText('No duplicates found! Your demos are clean.');
  const rows = page.locator('#audit-results-body tr.audit-file-row');
  await expect(rows.locator('.audit-file-path')).toHaveText(['C:\\demos\\solo.dem backed up in cup.rar']);
  await expect(page.locator('#audit-list-footer')).toHaveText('Duplicates Found: 0 | Wasted Space: 0.0 MB | Backed up in archives: 1');
  await expect(page.locator('#delete-selected-audits-btn')).toBeDisabled();
  await page.click('.audit-backed-up-row .group-toggle-btn');
  await expect(rows).toHaveCount(0);
  await expect(page.locator('.audit-backed-up-row .group-toggle-btn')).toHaveText('▶ Backed up in archives (1 demo)');
});
