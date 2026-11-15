// audit_duplicates.js — the Demo Auditor's Duplicates tab, the parts with
// no DOM (#844): which folders an audit walks, what its progress line says,
// which copies start ticked for deletion, and how a delete's result updates
// the groups. auditor_pane.js and audit_folders.js do the DOM.
import { STRINGS } from './strings.js';
import { againstLastRun, lastRunKey } from './progress_view.js';
import { fileNameOf } from './path_display.js';
import { gbOrMbText } from './byte_size.js';

const comparable = (p) => p.trim().replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase();

/// A path as Explorer shows it: no `\\?\` (or `\\?\UNC\`) prefix, which the
/// key pass's canonical paths carry.
export function shownPath(path) {
  if (path.startsWith('\\\\?\\UNC\\')) return `\\\\${path.slice(8)}`;
  if (path.startsWith('\\\\?\\')) return path.slice(4);
  return path;
}

/// `folders` plus `picked`, without blanks or a folder already there
/// (case-insensitive, trailing separator ignored).
export function addFolders(folders, picked) {
  const out = [...folders];
  const seen = new Set(out.map(comparable));
  for (const folder of [].concat(picked ?? [])) {
    if (typeof folder !== 'string' || !folder.trim()) continue;
    const key = comparable(folder);
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(folder);
  }
  return out;
}

/// The folders the audit walks: the Target Folder, then the added ones.
/// The backend adds every fixed drive itself when All drives is on.
export function auditFolders(targetFolder, extraFolders) {
  return addFolders(targetFolder?.trim() ? [targetFolder.trim()] : [], extraFolders);
}

/// Whether Start Audit has anything to walk.
export function canStartAudit(targetFolder, extraFolders, allDrives) {
  return allDrives || auditFolders(targetFolder, extraFolders).length > 0;
}

/// `All drives (C:, D:)` from `list_fixed_drives`' roots.
export function allDrivesLabel(roots) {
  const letters = (roots ?? []).map((r) => r.replace(/\\+$/, '')).filter(Boolean);
  return STRINGS.AUDITOR.allDrivesLabel(letters);
}

/// `audit_progress` → the bar's width and the step line. `total` is 0 while
/// folders are still being listed. Then the bar measures against
/// `expectedFolders`, how many the last audit of the same folders listed,
/// stopping short of full since this one may list more; with no earlier
/// audit there's nothing to measure against, and the bar moves without a
/// width (`indeterminate`).
export function auditProgressView(p, expectedFolders = 0) {
  const current = p?.current ? shownPath(p.current) : '';
  // Between the walk and the key pass, with "Show demos backed up in
  // archives" (#929).
  if (!p?.total && p?.zips_total > 0) {
    const done = Math.min(p.zips_done ?? 0, p.zips_total);
    return {
      pct: Math.round((done / p.zips_total) * 100),
      text: STRINGS.AUDITOR.zipsLine(done, p.zips_total),
      current,
      indeterminate: false,
    };
  }
  if (!p?.total) {
    const folders = p?.folders ?? 0;
    const bar = againstLastRun(folders, expectedFolders);
    const text = expectedFolders > 0
      ? STRINGS.AUDITOR.listingLineOf(folders, expectedFolders)
      : STRINGS.AUDITOR.listingLine(folders);
    return { ...bar, text, current };
  }
  const done = Math.min(p.done ?? 0, p.total);
  return {
    pct: Math.round((done / p.total) * 100),
    text: STRINGS.AUDITOR.keyingLine(done, p.total),
    current,
    indeterminate: false,
  };
}

/// The key the last audit's folder count is remembered under: the same
/// folders, in any order, or All drives.
export function auditFoldersKey(paths, allDrives) {
  return lastRunKey('audit_last_folders', paths, allDrives ? 'all' : '');
}

/// `audit_delete_progress` → the same bar, while copies are compared with
/// the kept ones (`step` "checking"), then deleted.
export function deleteProgressView(p, permanent) {
  const total = p?.total ?? 0;
  const done = Math.min(p?.done ?? 0, total);
  return {
    pct: total ? Math.round((done / total) * 100) : 0,
    text: p?.step === 'checking'
      ? STRINGS.AUDITOR.checkingCopiesLine(done, total)
      : STRINGS.AUDITOR.deletingLine(done, total, permanent),
    current: '',
  };
}

/// Ticked for deletion when the audit lists it: every copy but the kept
/// one (the first), except copies the game folder or the project uses. An
/// archive holding the demo changes nothing here (#929): it is a backup.
export function startsTicked(file, index) {
  return index > 0 && !file.in_use;
}

/// Whether `file` can be ticked to delete: a copy (not group's kept one)
/// and not marked as another demo.
export function canDelete(group, file) {
  return group.files[0]?.path !== file.path && !isNotCopy(group, file.path);
}

/// What the delete is sent: each ticked copy that can be deleted, with the
/// copy its group keeps, which it must match byte for byte.
export function copiesToDelete(groups, ticked) {
  return groups.flatMap((g) => {
    const kept = g.files[0];
    return g.files.slice(1)
      .filter((f) => ticked.has(f.path) && canDelete(g, f))
      .map((f) => ({ path: f.path, keep: kept.path }));
  });
}

/// The last part of an archive's path: `pack.7z`.
function archiveName(zip) {
  return fileNameOf(shownPath(zip));
}

/// The chip of a loose demo an archive holds a copy of (#929), or `null`:
/// its text names the archive, or how many when there are several, and its
/// tooltip lists every archive and entry. The wire name `zip` holds any of
/// .zip, .7z and .rar.
export function backupChip(file) {
  const backups = file?.backups ?? [];
  if (!backups.length) return null;
  const archives = [...new Set(backups.map((b) => b.zip))].map(archiveName);
  const entries = backups.map((b) => `${shownPath(b.zip)} › ${b.entry}`);
  return { text: STRINGS.AUDITOR.backedUpChip(archives), title: STRINGS.AUDITOR.backedUpTitle(entries) };
}

/// The backed-up demos (#929) still on disk after a delete.
export function backedUpWithout(backedUp, deleted) {
  const gone = new Set(deleted);
  return (backedUp ?? []).filter((f) => !gone.has(f.path));
}

/// The audit's groups, each remembering the audit's own pick (`auditPick`)
/// and its row order (`order`), so picking another copy to keep moves no row
/// and can be undone.
export function withAuditOrder(groups) {
  return (groups ?? []).map((g) => ({
    ...g,
    auditPick: g.files[0]?.path,
    order: g.files.map((f) => f.path),
  }));
}

/// Group `group`'s rows in the order the audit listed them, each with whether
/// it's the kept copy, which is always `files[0]`.
export function shownFiles(group) {
  const kept = group.files[0]?.path;
  const rank = new Map((group.order ?? []).map((p, i) => [p, i]));
  return [...group.files]
    .sort((a, b) => (rank.get(a.path) ?? Infinity) - (rank.get(b.path) ?? Infinity))
    .map((file) => ({ file, kept: file.path === kept }));
}

/// Whether the group keeps another copy than the audit picked.
export function keptChanged(group) {
  return !!group.auditPick && group.files[0]?.path !== group.auditPick
    && group.files.some((f) => f.path === group.auditPick);
}

/// The groups with `path` as the kept copy of group `groupIdx`: it moves to
/// the front, where the kept copy always is, and the rest keep their order.
/// Also says which ticks change: the new kept copy can't be deleted, and the
/// old one is a copy now, ticked by the usual rule (#844).
export function keepFile(groups, groupIdx, path) {
  const group = groups[groupIdx];
  const at = group?.files.findIndex((f) => f.path === path) ?? -1;
  if (at <= 0) return { groups, untick: null, tick: null };
  const files = [group.files[at], ...group.files.filter((_, i) => i !== at)];
  const was = group.files[0];
  // "Not an exact copy" was measured against the old pick, so it goes too.
  const out = groups.map((g, i) => (i === groupIdx ? { ...g, files, notCopies: [] } : g));
  return { groups: out, untick: path, tick: startsTicked(was, 1) ? was.path : null };
}

/// For each group with a copy inside `folder` (or a folder under it), that
/// copy: `{ groupIdx, path, already }`, `already` when it's the kept one now.
/// The first such copy in the group's order, which is the audit's ranking.
export function keepPicksIn(groups, folder) {
  const base = comparable(shownPath(folder ?? ''));
  if (!base) return [];
  const inside = (p) => comparable(shownPath(p)).startsWith(`${base}\\`);
  const picks = [];
  groups.forEach((g, groupIdx) => {
    const at = g.files.findIndex((f) => inside(f.path));
    if (at >= 0) picks.push({ groupIdx, path: g.files[at].path, already: at === 0 });
  });
  return picks;
}

/// The groups after a delete: `deleted` paths gone, and a group left with
/// one file dropped, since it has no copies any more.
export function withoutDeleted(groups, deleted) {
  const gone = new Set(deleted);
  return groups
    .map((g) => ({ ...g, files: g.files.filter((f) => !gone.has(f.path)) }))
    .filter((g) => g.files.length > 1);
}

/// The groups with each of `paths` marked as not an exact copy of its
/// group's kept copy: a delete compared them and found other bytes, so they
/// are different demos that only share the audit's key.
export function markNotCopies(groups, paths) {
  const marked = new Set(paths);
  if (!marked.size) return groups;
  return groups.map((g) => {
    const add = g.files.slice(1).map((f) => f.path).filter((p) => marked.has(p));
    return add.length ? { ...g, notCopies: [...new Set([...(g.notCopies ?? []), ...add])] } : g;
  });
}

/// Whether `path` is marked not an exact copy in `group`.
export function isNotCopy(group, path) {
  return (group.notCopies ?? []).includes(path);
}

/// Copies and the space they take, over every group. A file marked not an
/// exact copy isn't one.
export function auditTotals(groups) {
  let copies = 0;
  let wastedBytes = 0;
  for (const g of groups) {
    const n = g.files.slice(1).filter((f) => !isNotCopy(g, f.path)).length;
    copies += n;
    wastedBytes += g.size * n;
  }
  return { copies, wastedBytes };
}

/** Wasted space for the footer: GB from 1 GB up, MB below it. */
export function wastedSizeText(bytes) {
  return gbOrMbText(bytes);
}

/// The Status cell of a file row: kept, in use, or a copy.
export function fileStatus(file, index) {
  const why = file.in_use ? STRINGS.AUDITOR.IN_USE[file.in_use] : '';
  if (index === 0) return why ? STRINGS.AUDITOR.keptBecause(why) : STRINGS.AUDITOR.KEPT_LABEL;
  return why ? STRINGS.AUDITOR.inUseCopy(why) : STRINGS.AUDITOR.COPY_LABEL;
}
