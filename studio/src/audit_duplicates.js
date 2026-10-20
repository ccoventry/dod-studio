// audit_duplicates.js — the Demo Auditor's Duplicates tab, the parts with
// no DOM (#844): which folders an audit walks, what its progress line says,
// which copies start ticked for deletion, and how a delete's result updates
// the groups. auditor_pane.js and audit_folders.js do the DOM.
import { STRINGS } from './strings.js';

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
/// folders are still being listed, when there's nothing to measure against.
export function auditProgressView(p) {
  const current = p?.current ? shownPath(p.current) : '';
  if (!p?.total) {
    return { pct: 0, text: STRINGS.AUDITOR.listingLine(p?.folders ?? 0), current };
  }
  const done = Math.min(p.done ?? 0, p.total);
  return {
    pct: Math.round((done / p.total) * 100),
    text: STRINGS.AUDITOR.keyingLine(done, p.total),
    current,
  };
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
/// one (the first), except copies the game folder or the project uses.
export function startsTicked(file, index) {
  return index > 0 && !file.in_use;
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
  const out = groups.map((g, i) => (i === groupIdx ? { ...g, files } : g));
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

/// Copies and the space they take, over every group.
export function auditTotals(groups) {
  let copies = 0;
  let wastedBytes = 0;
  for (const g of groups) {
    copies += g.files.length - 1;
    wastedBytes += g.size * (g.files.length - 1);
  }
  return { copies, wastedBytes };
}

/** Wasted space for the footer: GB from 1 GB up, MB below it. */
export function wastedSizeText(bytes) {
  const gb = bytes / (1024 * 1024 * 1024);
  return gb >= 1 ? `${gb.toFixed(2)} GB` : `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/// The Status cell of a file row: kept, in use, or a copy.
export function fileStatus(file, index) {
  const why = file.in_use ? STRINGS.AUDITOR.IN_USE[file.in_use] : '';
  if (index === 0) return why ? STRINGS.AUDITOR.keptBecause(why) : STRINGS.AUDITOR.KEPT_LABEL;
  return why ? STRINGS.AUDITOR.inUseCopy(why) : STRINGS.AUDITOR.COPY_LABEL;
}
