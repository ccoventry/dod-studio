import { describe, it, expect } from 'vitest';
import {
  shownPath, addFolders, auditFolders, canStartAudit, allDrivesLabel, auditProgressView,
  deleteProgressView, startsTicked, withoutDeleted, auditTotals, wastedSizeText, fileStatus, keepFile, keepPicksIn,
  withAuditOrder, shownFiles, keptChanged, markNotCopies, isNotCopy, auditFoldersKey,
  isArchived, canDelete, copiesToDelete, revealPath,
} from './audit_duplicates.js';

describe('audit_duplicates (#844)', () => {
  it('shows paths without the verbatim prefix', () => {
    expect(shownPath('\\\\?\\C:\\demos\\a.dem')).toBe('C:\\demos\\a.dem');
    expect(shownPath('\\\\?\\UNC\\nas\\d\\a.dem')).toBe('\\\\nas\\d\\a.dem');
    expect(shownPath('C:\\a.dem')).toBe('C:\\a.dem');
  });

  it('adds picked folders once, ignoring case, slashes and blanks', () => {
    expect(addFolders(['C:\\Demos'], ['c:\\demos\\', 'F:\\', ' ', null, 'F:/'])).toEqual(['C:\\Demos', 'F:\\']);
    expect(addFolders([], 'D:\\one')).toEqual(['D:\\one']);
    expect(addFolders(['D:\\one'], null)).toEqual(['D:\\one']);
  });

  it('puts the Target Folder first and can start with folders or All drives', () => {
    expect(auditFolders(' C:\\t ', ['F:\\x', 'c:\\t'])).toEqual(['C:\\t', 'F:\\x']);
    expect(auditFolders('', ['F:\\x'])).toEqual(['F:\\x']);
    expect(canStartAudit('', [], false)).toBe(false);
    expect(canStartAudit('', [], true)).toBe(true);
    expect(canStartAudit('', ['F:\\x'], false)).toBe(true);
  });

  it('names the drives All drives will check', () => {
    expect(allDrivesLabel(['C:\\', 'F:\\'])).toBe('All drives (C:, F:)');
    expect(allDrivesLabel([])).toBe('All drives');
    expect(allDrivesLabel(undefined)).toBe('All drives');
  });

  it('says which step the audit is on, with a bar once there is a total', () => {
    expect(auditProgressView(null)).toEqual({
      pct: 0, text: 'Looking for demos: 0 folders read', current: '', indeterminate: true,
    });
    expect(auditProgressView({ folders: 1, total: 0, done: 0, current: '\\\\?\\C:\\x' }))
      .toEqual({ pct: 0, text: 'Looking for demos: 1 folder read', current: 'C:\\x', indeterminate: true });
    expect(auditProgressView({ folders: 9, total: 8, done: 2, current: 'C:\\a.dem' }))
      .toEqual({ pct: 25, text: 'Comparing demos: 2 / 8', current: 'C:\\a.dem', indeterminate: false });
    expect(auditProgressView({ folders: 9, total: 8, done: 12 }).pct).toBe(100);
  });

  it('measures the folder walk against the last audit of the same folders', () => {
    const view = auditProgressView({ folders: 93_000, total: 0 }, 186_000);
    expect(view.pct).toBe(50);
    expect(view.indeterminate).toBe(false);
    expect(view.text).toBe('Looking for demos: 93000 folders read (about 186000 last time)');
    // More folders than last time: the bar waits short of full.
    expect(auditProgressView({ folders: 200_000, total: 0 }, 186_000).pct).toBe(95);
  });

  it('keys the remembered count by its folders', () => {
    // The key #901 stored counts under, so they still count after #951.
    expect(auditFoldersKey(['D:\\b', 'C:\\A'], true)).toBe('audit_last_folders:all|c:\\a|d:\\b');
    expect(auditFoldersKey(['D:\\b', 'C:\\A'], false)).toBe(auditFoldersKey(['c:\\a', 'd:\\B'], false));
    expect(auditFoldersKey([], true)).not.toBe(auditFoldersKey([], false));
  });

  it('words the delete progress by where the files go', () => {
    expect(deleteProgressView({ done: 1, total: 4 }, false).text).toBe('Moving to the Recycle Bin: 1 / 4');
    expect(deleteProgressView({ done: 4, total: 4 }, true)).toEqual({ pct: 100, text: 'Deleting permanently: 4 / 4', current: '' });
    expect(deleteProgressView({ done: 0, total: 0 }, false).pct).toBe(0);
    expect(deleteProgressView({ step: 'checking', done: 1, total: 4 }, false).text)
      .toBe('Checking each copy matches the kept one byte for byte: 1 / 4');
  });

  it('never ticks the kept copy or a copy the game or the project uses', () => {
    expect(startsTicked({ path: 'a', in_use: null }, 0)).toBe(false);
    expect(startsTicked({ path: 'b', in_use: null }, 1)).toBe(true);
    expect(startsTicked({ path: 'c', in_use: 'queue' }, 2)).toBe(false);
  });

  it('drops deleted copies and groups left with one file', () => {
    const groups = [
      { size: 10, header_hash: 1, files: [{ path: 'a1' }, { path: 'a2' }] },
      { size: 20, header_hash: 2, files: [{ path: 'b1' }, { path: 'b2' }, { path: 'b3' }] },
    ];
    expect(withoutDeleted(groups, ['a2', 'b3'])).toEqual([
      { size: 20, header_hash: 2, files: [{ path: 'b1' }, { path: 'b2' }] },
    ]);
    expect(auditTotals(groups)).toEqual({ copies: 3, wastedBytes: 50 });
  });

  it('labels each row kept, a copy, or a copy in use', () => {
    expect(fileStatus({ in_use: null }, 0)).toBe('Kept');
    expect(fileStatus({ in_use: 'game_folder' }, 0)).toBe('Kept (in the game folder)');
    expect(fileStatus({ in_use: null }, 1)).toBe('   ↳ Copy');
    expect(fileStatus({ in_use: 'queue' }, 1)).toBe("   ↳ Copy, in this project's queue");
  });

  it('says wasted space in MB under a gigabyte, GB from there', () => {
    expect(wastedSizeText(21_870_000)).toBe('20.9 MB');
    expect(wastedSizeText(3 * 1024 ** 3)).toBe('3.00 GB');
    expect(wastedSizeText(0)).toBe('0.0 MB');
  });

  it('keeps another copy: it moves to the front, the old pick becomes a ticked copy', () => {
    const [d, e, f] = [String.raw`D:\a.dem`, String.raw`E:\a.dem`, String.raw`F:\a.dem`];
    const groups = [{ size: 1, header_hash: 1, files: [{ path: d, in_use: null }, { path: e, in_use: null }, { path: f, in_use: null }] }];
    const r = keepFile(groups, 0, f);
    expect(r.groups[0].files.map((x) => x.path)).toEqual([f, d, e]);
    expect(r.untick).toBe(f);
    expect(r.tick).toBe(d);
    // The input is left alone, and keeping the kept one changes nothing.
    expect(groups[0].files[0].path).toBe(d);
    expect(keepFile(groups, 0, d).untick).toBeNull();
  });

  it('shows rows in the audit order whichever copy is kept, and knows when the pick changed', () => {
    const [d, e, f] = [String.raw`D:\a.dem`, String.raw`E:\a.dem`, String.raw`F:\a.dem`];
    const groups = withAuditOrder([{ size: 1, header_hash: 1, files: [{ path: d }, { path: e }, { path: f }] }]);
    expect(keptChanged(groups[0])).toBe(false);
    const changed = keepFile(groups, 0, f).groups;
    expect(shownFiles(changed[0]).map(({ file, kept }) => [file.path, kept])).toEqual([[d, false], [e, false], [f, true]]);
    expect(keptChanged(changed[0])).toBe(true);
    expect(keptChanged(keepFile(changed, 0, d).groups[0])).toBe(false);
    // The audit's pick deleted: nothing to go back to.
    expect(keptChanged(withoutDeleted(changed, [d])[0])).toBe(false);
  });

  it('marks a copy the delete found different, counts it out, and clears the mark when the pick changes', () => {
    const groups = [{ size: 10, header_hash: 1, files: [{ path: 'a' }, { path: 'b' }, { path: 'c' }] }];
    const marked = markNotCopies(groups, ['c', 'a', 'elsewhere']);
    // The kept copy is never marked.
    expect(marked[0].notCopies).toEqual(['c']);
    expect(isNotCopy(marked[0], 'c')).toBe(true);
    expect(auditTotals(marked)).toEqual({ copies: 1, wastedBytes: 10 });
    expect(markNotCopies(groups, [])).toBe(groups);
    expect(keepFile(marked, 0, 'b').groups[0].notCopies).toEqual([]);
  });

  it('an old pick the game uses is not ticked when another copy is kept', () => {
    const e = String.raw`E:\a.dem`;
    const groups = [{ size: 1, header_hash: 1, files: [{ path: String.raw`C:\hl\dod\a.dem`, in_use: 'game_folder' }, { path: e, in_use: null }] }];
    expect(keepFile(groups, 0, e).tick).toBeNull();
  });

  it("finds each group's copy inside a folder, verbatim paths and case aside", () => {
    const groups = [
      { size: 1, header_hash: 1, files: [{ path: String.raw`\\?\D:\a.dem` }, { path: String.raw`\\?\F:\Keep\sub\a.dem` }] },
      { size: 1, header_hash: 2, files: [{ path: String.raw`\\?\F:\keep\b.dem` }, { path: String.raw`\\?\G:\b.dem` }] },
      { size: 1, header_hash: 3, files: [{ path: String.raw`\\?\F:\Keeper\c.dem` }, { path: String.raw`\\?\G:\c.dem` }] },
    ];
    expect(keepPicksIn(groups, String.raw`f:\keep\ `.trim())).toEqual([
      { groupIdx: 0, path: String.raw`\\?\F:\Keep\sub\a.dem`, already: false },
      { groupIdx: 1, path: String.raw`\\?\F:\keep\b.dem`, already: true },
    ]);
    expect(keepPicksIn(groups, '')).toEqual([]);
  });
});

describe('demos inside zips (#929)', () => {
  const zipped = {
    path: String.raw`\\?\D:\packs\lan.zip › hltv/m.dem`,
    in_use: null,
    archive: { zip: String.raw`\\?\D:\packs\lan.zip`, entry: 'hltv/m.dem' },
  };
  const loose = { path: String.raw`\\?\C:\demos\m.dem`, in_use: null };
  const other = { path: String.raw`\\?\E:\m.dem`, in_use: null };
  const group = { size: 10, header_hash: 1, files: [zipped, loose, other] };

  it('never ticks or deletes a copy inside a zip', () => {
    expect(isArchived(zipped)).toBe(true);
    expect(isArchived(loose)).toBe(false);
    expect(startsTicked(zipped, 1)).toBe(false);
    expect(startsTicked(loose, 1)).toBe(true);
    expect(canDelete(group, zipped)).toBe(false);
    expect(canDelete(group, loose)).toBe(true);
    // Another copy kept: the zip's copy is still not one to delete.
    const { groups } = keepFile([group], 0, loose.path);
    expect(canDelete(groups[0], zipped)).toBe(false);
  });

  it('sends the zip and entry when the kept copy is inside a zip', () => {
    const ticked = new Set([loose.path, other.path, zipped.path]);
    expect(copiesToDelete([group], ticked)).toEqual([
      { path: loose.path, keep: zipped.path, keep_zip: zipped.archive },
      { path: other.path, keep: zipped.path, keep_zip: zipped.archive },
    ]);
    // A loose copy kept: no zip to name, and the zip's copy isn't sent.
    const { groups } = keepFile([group], 0, loose.path);
    expect(copiesToDelete(groups, ticked)).toEqual([{ path: other.path, keep: loose.path }]);
    expect(copiesToDelete([group], new Set())).toEqual([]);
  });

  it('counts only the loose copies as space a delete frees', () => {
    expect(auditTotals([group])).toEqual({ copies: 2, wastedBytes: 20 });
    const twoZips = { size: 10, files: [zipped, { ...zipped, path: 'x › y.dem', archive: { zip: 'x', entry: 'y.dem' } }] };
    expect(auditTotals([twoZips])).toEqual({ copies: 0, wastedBytes: 0 });
  });

  it('opens the zip itself in Explorer', () => {
    expect(revealPath(zipped)).toBe(String.raw`D:\packs\lan.zip`);
    expect(revealPath(loose)).toBe(String.raw`C:\demos\m.dem`);
  });

  it('shows how many archives have been looked inside before the key pass', () => {
    expect(auditProgressView({ folders: 9, zips_total: 4, zips_done: 1, total: 0, done: 0, current: String.raw`\\?\D:\a.7z` }))
      .toEqual({ pct: 25, text: 'Looking inside archives: 1 / 4', current: String.raw`D:\a.7z`, indeterminate: false });
    // The key pass takes over once it has a total.
    expect(auditProgressView({ folders: 9, zips_total: 4, zips_done: 4, total: 10, done: 5 }).text)
      .toBe('Comparing demos: 5 / 10');
  });
});
