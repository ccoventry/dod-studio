// batch_results.js
// The Last Batch panel under Highlight Details (#172, decided as D7 in the
// 2026-09-28 review): shown when a capture batch ends and kept until the next
// one starts, so what happened can be checked after walking away from it.
// How it ended, then one row per take: which highlight, whether it landed
// (and whether Render Studio can use it), its size, and its folder.
//
// Fed by capture_pane.js: `batchStarted()` when a batch starts,
// `batchEnded(kind, text)` from capture_status, and `batchVerified(payload,
// dispatch)` from capture_takes_verified, in whichever order they arrive.
// In OBS mode, `batchObsClip(report)` from obs_drop_rate.js adds what each
// take lost to OBS's frame drops.

import { revealInExplorer } from './ipc_bridge.js';
import { highlightStartSeconds } from './highlight_time.js';
import { clockFloor } from './time_format.js';
import { STRINGS } from './strings.js';

let state = null; // { endedAt, kind, text, rows, totals } once a batch ends
let obsClips = []; // OBS mode: each clip's closing drop-rate report, in order

/** A take folder as either spelling of it compares. */
function folderKey(path) {
  return String(path || '').replaceAll('\\', '/').replace(/\/+$/, '').toLowerCase();
}

/** Each row's OBS report, or null: matched by take folder, else by order
 *  (OBS records the takes in the order the panel lists them). */
export function obsForRows(rows, clips) {
  if (!clips?.length) return rows.map(() => null);
  const byFolder = new Map(clips.filter((c) => c.take_folder).map((c) => [folderKey(c.take_folder), c]));
  return rows.map((row, i) => byFolder.get(folderKey(row.folder))
    ?? clips.find((c) => !c.take_folder && c.clip === i + 1)
    ?? null);
}

function panel() {
  return document.querySelector('#batch-results');
}

/** Bytes as MB or GB. */
export function formatBytes(bytes) {
  const gb = bytes / 1024 ** 3;
  return gb >= 1 ? `${gb.toFixed(2)} GB` : `${(bytes / 1024 ** 2).toFixed(1)} MB`;
}

/** What a take covered: the first highlight's demo, player, kills and time,
 *  and how many more were merged into it. */
export function takeLabel(block, dispatch) {
  const indices = block.source_streak_indices || [];
  const streak = dispatch?.streaks?.[indices[0]];
  if (!streak) return block.demo_name || block.take_key;
  const parts = [block.demo_name];
  if (streak.target_player) parts.push(streak.target_player);
  parts.push(STRINGS.BATCH_RESULTS.kills(streak.kill_count));
  parts.push(clockFloor(highlightStartSeconds(streak)));
  const label = parts.filter(Boolean).join(' · ');
  return indices.length > 1 ? `${label} ${STRINGS.BATCH_RESULTS.merged(indices.length - 1)}` : label;
}

/** The panel's rows and totals from a capture_takes_verified payload. */
export function resultsOf(payload, dispatch) {
  const blocks = payload?.blocks || [];
  const rows = blocks.map((b) => ({
    label: takeLabel(b, dispatch),
    // cut_short: a partial take the batch's end left on disk (#1081), not a
    // clip, with what ended it (`cut_by`); short: one FFmpeg stopped writing
    // mid-clip (#1105).
    status: b.short_take ? 'short' : b.cut_short ? 'cut_short' : !b.captured ? 'missing' : b.renderable ? 'ok' : 'unrenderable',
    short: b.short_take || null,
    cutBy: b.cut_by || null,
    bytes: b.bytes || 0,
    folder: b.take_folder,
  }));
  return {
    rows,
    totals: {
      takes: blocks.length,
      captured: blocks.filter((b) => b.captured).length,
      renderable: blocks.filter((b) => b.renderable).length,
      bytes: rows.reduce((n, r) => n + r.bytes, 0),
    },
  };
}

function render() {
  const el = panel();
  if (!el) return;
  if (!state) {
    el.hidden = true;
    el.replaceChildren();
    return;
  }
  const S = STRINGS.BATCH_RESULTS;
  el.hidden = false;
  const head = document.createElement('div');
  head.className = 'batch-results-head';
  const title = document.createElement('strong');
  title.textContent = S.title(state.endedAt.toLocaleTimeString());
  const close = document.createElement('button');
  close.type = 'button';
  close.className = 'batch-results-close';
  close.textContent = '✕';
  close.title = S.DISMISS;
  close.addEventListener('click', () => {
    state = null;
    render();
  });
  head.append(title, close);

  const summary = document.createElement('p');
  summary.className = `batch-results-summary batch-results-${state.kind}`;
  const outcome = state.kind ? S.outcome(state.kind, state.text) : '';
  const totals = state.totals
    ? S.totals(state.totals.captured, state.totals.takes, formatBytes(state.totals.bytes), state.totals.captured - state.totals.renderable)
    : S.CHECKING;
  const obs = obsForRows(state.rows || [], obsClips);
  const obsMeasured = obs.filter(Boolean);
  const obsLagging = obsMeasured.filter((o) => o.lagging);
  const obsLine = obsLagging.length
    ? S.obsSummary(obsLagging.length, obsMeasured.length, STRINGS.OBS_DROP_RATE.fix(obsLagging[0].capture_fps))
    : '';
  summary.textContent = [outcome, totals, obsLine].filter(Boolean).join(' ');

  const children = [head, summary];
  if (state.rows?.length) {
    const table = document.createElement('table');
    table.className = 'analyzer-table batch-results-table';
    const body = document.createElement('tbody');
    for (const [i, row] of state.rows.entries()) {
      const tr = document.createElement('tr');
      tr.className = `batch-results-row-${row.status}`;
      const status = document.createElement('td');
      status.textContent = row.short
        ? S.shortTake(row.short.video_secs, row.short.audio_secs)
        : row.status === 'cut_short' ? S.cutShortBy(row.cutBy) : S.STATUS[row.status];
      const label = document.createElement('td');
      label.textContent = row.label;
      const size = document.createElement('td');
      size.textContent = row.status === 'missing' ? '—' : formatBytes(row.bytes);
      const open = document.createElement('td');
      if (row.status !== 'missing' && row.folder) {
        const btn = document.createElement('button');
        btn.type = 'button';
        btn.textContent = S.OPEN_FOLDER;
        btn.addEventListener('click', () => revealInExplorer(row.folder).catch(() => {}));
        open.append(btn);
      }
      const cells = [status, label, size];
      if (obsMeasured.length) {
        const dropped = document.createElement('td');
        const o = obs[i];
        if (o) {
          dropped.textContent = S.obsCell(o.skipped_pct, o.missed_pct);
          dropped.title = S.obsCellTitle(o.skipped, o.frames, o.missed, o.rendered);
          if (o.lagging) dropped.className = 'batch-results-obs-lagging';
        }
        cells.push(dropped);
      }
      tr.append(...cells, open);
      body.append(tr);
    }
    table.append(body);
    const wrap = document.createElement('div');
    wrap.className = 'table-wrapper batch-results-wrap';
    wrap.append(table);
    children.push(wrap);
  }
  el.replaceChildren(...children);
}

/** A new batch: the last one's results go. */
export function batchStarted() {
  state = null;
  obsClips = [];
  render();
}

/** How the batch ended: `kind` is 'completed', 'cancelled' or 'error'. */
export function batchEnded(kind, text) {
  state = { ...(state || {}), endedAt: new Date(), kind, text: text || '' };
  render();
}

/** What landed on disk, from capture_takes_verified. */
export function batchVerified(payload, dispatch) {
  state = { endedAt: new Date(), ...(state || {}), ...resultsOf(payload, dispatch) };
  render();
}

/** OBS mode: one clip's totals, from its closing drop-rate report. */
export function batchObsClip(report) {
  obsClips.push(report);
  if (state) render();
}
