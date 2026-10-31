// split_status.js
// Each ticked map's row while Split Checked Maps runs (#876), like the Render
// tab's jobs: a Status that moves Waiting → Reading the demo → Writing →
// Checking → Finished (or Failed, or Cancelled), its own progress, and the
// file once it's written. Pure: split_pane.js draws it. Fed native's
// `SplitProgress` events (`split_progress`), then the split's outcome.

import { STRINGS } from './strings.js';

/** The stages a map's row ends at; nothing moves it on from one. */
export const FINAL_STAGES = new Set(['finished', 'failed', 'cancelled']);

const pctOf = (f) => Math.max(0, Math.min(100, Math.round((Number(f) || 0) * 100)));

/** Rows for the ticked maps `keep` (map indexes) as the split starts. */
export function startRows(keep) {
  const rows = {};
  for (const i of keep) rows[i] = { stage: 'waiting', pct: 0, bar: 0, file: null, error: null };
  return rows;
}

/**
 * `rows` after one `SplitProgress`. The read moves every row still going to
 * Reading; an event for one map moves that row. A row that has ended stays
 * as it ended. `pct` is the stage's own, for the Status: how far through
 * the read, or through the check (the second half of a map's
 * `map_fraction`). `bar` is the map's own write and check, for its bar.
 */
export function applyProgress(rows, p) {
  if (!p) return rows;
  const next = { ...rows };
  if (p.stage === 'reading') {
    for (const [i, row] of Object.entries(rows)) {
      if (!FINAL_STAGES.has(row.stage)) next[i] = { ...row, stage: 'reading', pct: pctOf(p.map_fraction) };
    }
    return next;
  }
  const row = rows[p.index];
  if (!row || FINAL_STAGES.has(row.stage)) return rows;
  const pct = p.stage === 'checking'
    ? pctOf(((p.map_fraction ?? 0.5) - 0.5) * 2)
    : FINAL_STAGES.has(p.stage) ? 100 : 0;
  // A cancelled map's bar stays where it stopped: a full one reads as written.
  const bar = p.stage === 'cancelled' ? row.bar
    : FINAL_STAGES.has(p.stage) ? 100 : pctOf(p.map_fraction);
  next[p.index] = { stage: p.stage, pct, bar, file: p.file ?? row.file, error: p.error ?? row.error };
  return next;
}

/**
 * `rows` once the split returns `outcome` ({ written, failed, cancelled }).
 * A row the events left going ends as the outcome says: written, failed,
 * or else cancelled when Cancel stopped the split.
 */
export function applyOutcome(rows, outcome) {
  const next = { ...rows };
  for (const w of outcome?.written ?? []) {
    if (next[w.index]) next[w.index] = { ...next[w.index], stage: 'finished', pct: 100, bar: 100, file: w };
  }
  for (const f of outcome?.failed ?? []) {
    if (next[f.index]) next[f.index] = { ...next[f.index], stage: 'failed', pct: 100, bar: 100, error: f.error };
  }
  for (const [i, row] of Object.entries(next)) {
    if (FINAL_STAGES.has(row.stage)) continue;
    next[i] = outcome?.cancelled
      ? { ...row, stage: 'cancelled' }
      : { ...row, stage: 'failed', error: STRINGS.SPLIT.NOT_WRITTEN };
  }
  return next;
}

/** Every row still going fails with `err`: the split itself failed. */
export function applyError(rows, err) {
  const next = { ...rows };
  for (const [i, row] of Object.entries(rows)) {
    if (!FINAL_STAGES.has(row.stage)) next[i] = { ...row, stage: 'failed', error: String(err) };
  }
  return next;
}

/** A row's Status cell. */
export function statusText(row) {
  const S = STRINGS.SPLIT;
  switch (row?.stage) {
    case 'waiting': return S.STATUS_WAITING;
    case 'reading': return S.statusReading(row.pct);
    case 'writing': return S.STATUS_WRITING;
    case 'checking': return S.statusChecking(row.pct);
    case 'finished': return S.STATUS_FINISHED;
    case 'failed': return S.statusFailed(row.error);
    case 'cancelled': return S.STATUS_CANCELLED;
    default: return '';
  }
}

/** The line under the button once a split has ended. */
export function summaryText(rows) {
  const all = Object.values(rows);
  const count = (stage) => all.filter((r) => r.stage === stage).length;
  return STRINGS.SPLIT.splitSummary(count('finished'), count('failed'), count('cancelled'));
}
