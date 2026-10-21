// audit_consolidate.js — the Demo Auditor's Consolidate (#872), the parts
// with no DOM: what the request sends, what the preview and progress lines
// say. audit_consolidate_ui.js does the DOM; native::demo_consolidate plans
// and moves.
import { STRINGS } from './strings.js';
import { wastedSizeText, isNotCopy } from './audit_duplicates.js';

const asDemo = (file, size) => ({ path: file.path, size, game_folder: file.in_use === 'game_folder' });

/// What `consolidate_plan` / `consolidate_run` take: each group's kept copy
/// (its first file) and every demo without copies move; the group's other
/// copies are recycled once the backend finds each matches its `keep` byte
/// for byte. A row already marked "Not an exact copy" is a different demo,
/// so it is never sent to be recycled.
export function consolidateRequest({ groups, singles, roots, destination, layout, includeGameFolders }) {
  const keep = [];
  const recycle = [];
  for (const g of groups ?? []) {
    if (!g.files?.length) continue;
    keep.push(asDemo(g.files[0], g.size));
    g.files.slice(1)
      .filter((f) => !isNotCopy(g, f.path))
      .forEach((f) => recycle.push({ ...asDemo(f, g.size), keep: g.files[0].path }));
  }
  (singles ?? []).forEach((s) => keep.push(asDemo(s, s.size)));
  return {
    destination: destination ?? '',
    layout: layout === 'subfolders' ? 'subfolders' : 'flat',
    include_game_folders: !!includeGameFolders,
    roots: roots ?? [],
    keep,
    recycle,
  };
}

/// Whether there is anything for Consolidate to work on.
export function hasDemos(groups, singles) {
  return (groups?.length ?? 0) + (singles?.length ?? 0) > 0;
}

/// The preview line for a plan, and whether Move Demos can start.
export function planView(plan) {
  if (!plan) return { text: '', ready: false };
  if (!plan.enough_space) {
    return {
      text: STRINGS.AUDITOR.consolidateNoSpace(wastedSizeText(plan.copy_bytes), wastedSizeText(plan.free_bytes)),
      ready: false,
    };
  }
  if (plan.moves.length === 0 && plan.recycle.length === 0) {
    return { text: STRINGS.AUDITOR.NOTHING_TO_CONSOLIDATE, ready: false };
  }
  return { text: STRINGS.AUDITOR.consolidatePreview(plan, wastedSizeText(plan.move_bytes)), ready: true };
}

/// `consolidate_progress` → the bar's width and the step line. Moves are
/// measured in bytes (a copy between drives is the slow part), checking
/// and recycling in files.
export function consolidateProgressView(p) {
  const files = Math.min(p?.files_done ?? 0, p?.files_total ?? 0);
  const total = p?.files_total ?? 0;
  if (p?.step === 'checking') {
    return {
      pct: total ? Math.round((files / total) * 100) : 0,
      text: STRINGS.AUDITOR.checkingCopiesLine(files, total),
      current: p?.current ?? '',
    };
  }
  if (p?.step === 'recycling') {
    return {
      pct: total ? Math.round((files / total) * 100) : 0,
      text: STRINGS.AUDITOR.recyclingLine(files, total),
      current: '',
    };
  }
  const bytesTotal = p?.bytes_total ?? 0;
  const bytes = Math.min(p?.bytes_done ?? 0, bytesTotal);
  const line = p?.step === 'undoing' ? STRINGS.AUDITOR.undoingLine : STRINGS.AUDITOR.movingLine;
  return {
    pct: bytesTotal ? Math.round((bytes / bytesTotal) * 100) : 0,
    text: line(files, total, wastedSizeText(bytes), wastedSizeText(bytesTotal)),
    current: p?.current ?? '',
  };
}

/// Demos a delete left without copies (`groups` before it): they join the
/// demos without copies, so Consolidate still moves them.
export function singlesAfterDelete(groups, deleted) {
  const gone = new Set(deleted);
  const out = [];
  for (const g of groups ?? []) {
    const left = g.files.filter((f) => !gone.has(f.path));
    if (left.length === 1 && left.length < g.files.length) {
      out.push({ path: left[0].path, size: g.size, in_use: left[0].in_use ?? null });
    }
  }
  return out;
}
