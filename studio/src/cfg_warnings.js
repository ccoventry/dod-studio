// cfg_warnings.js
// Tells the user what their own config files set, and what the app will
// override in them.
//
// Two different problems, both invisible without this:
//
//   1. A config sets something the pipeline reads and the app never hears about
//      it. That is how a capture ran at `mirv_fov 105` from movie.cfg while the
//      flush sized its on-screen test for the default 90.
//   2. The same cvar is given different values across the configs, Initial
//      Commands (the app's own additions included) and Scheduled Commands
//      before a clip. The last one wins, so the rest quietly stop applying
//      (#216's Rule 1). And a Scheduled After with no Before for that cvar
//      leaves its value in place for every later clip (Rule 2).
//
// ADVISORY ONLY. Nothing here, or anywhere in this app, writes to a config file.

import { scanGameConfigs } from './ipc_bridge.js';
import { STRINGS } from './strings.js';

const EMPTY = {
  unseen: [],
  conflicts: [],
  asymmetric: [],
  custom: [],
  bannedInit: [],
  bannedScheduled: [],
  tooLongInit: [],
  tooLongScheduled: [],
  decalDefaultRing: null,
  decalFlushIsNoop: false,
  noopInit: [],
  noopScheduled: [],
  fatalCvars: [],
  configCfgWritable: false,
};

let report = EMPTY;

/**
 * How many banned commands (see `cfg_scan::BANNED_COMMANDS`) and commands
 * too long to fit a demo frame (#453) the most recently fetched report
 * found, across Initial and Scheduled Commands combined. `refreshLaunchGuard` (capture_pane.js) reads this to block Start
 * Capture Batch — reflects whatever `refreshCfgWarnings` last resolved, so
 * it can go briefly stale between an edit and the next scan finishing, same
 * as every other advisory here.
 */
export function bannedCommandCount() {
  return (
    (report?.bannedInit?.length ?? 0) +
    (report?.bannedScheduled?.length ?? 0) +
    (report?.tooLongInit?.length ?? 0) +
    (report?.tooLongScheduled?.length ?? 0)
  );
}

/**
 * Re-scan and redraw. Safe to call repeatedly — on start-up, when the hl.exe
 * path changes, and whenever the init command list is edited.
 *
 * `context` carries the settings that decide what the app appends for itself,
 * so the overrides it reports are the ones a capture would really apply.
 *
 * `customCommands` are `{command, relation, offset_seconds}` rather than bare
 * strings, because the order the engine reaches them decides which one is
 * actually displacing a config value and which is just changing it again.
 */
export async function refreshCfgWarnings(
  gamePath,
  initCommands = [],
  customCommands = [],
  context = {}
) {
  report = gamePath
    ? await scanGameConfigs(gamePath, initCommands, customCommands, context)
    : EMPTY;
  render();
}

/** The cvar half of `cvar value` — `b.command` is the full typed line
 *  (arguments and all), but BANNED_REASONS is keyed by the bare cvar. */
function cvarOf(command) {
  return String(command).trim().split(/\s+/)[0] ?? '';
}

/** Where a stated value came from (map_manager.rs's ValueSourceRow). */
function describeSource(v, cvar) {
  switch (v.kind) {
    case 'config':
      return STRINGS.CFG.sourceConfig(v.file, v.line);
    case 'initial':
      return STRINGS.CFG.SOURCE_INITIAL;
    case 'app':
      return STRINGS.CFG.sourceApp(STRINGS.CFG.SETTING_FOR_CVAR[cvar.toLowerCase()] || STRINGS.CFG.UNKNOWN_SETTING);
    case 'before':
      return STRINGS.CFG.sourceBefore(v.offsetSeconds);
    case 'after':
      return STRINGS.CFG.sourceAfter(v.offsetSeconds);
    default:
      return v.kind;
  }
}

/** Rule 1 rows (#216): every value, where it came from, and which applies. */
function conflictSection(rows) {
  const items = rows
    .map((c) => {
      const values = c.values.map((v) => STRINGS.CFG.stated(v.value, describeSource(v, c.cvar))).join(', ');
      return `<li><code>${STRINGS.CFG.conflictRow(c.cvar, values, c.effective.value)}</code></li>`;
    })
    .join('');
  return section(STRINGS.CFG.CONFLICT_TITLE, STRINGS.CFG.CONFLICT_ADVICE, items, '#ff8a5c');
}

/** Commands too long for one demo ConsoleCommand frame (#453) — blocking. */
function tooLongSection(commands) {
  const rows = commands
    .map((c) => `<li><code>${STRINGS.CFG.tooLongRow(c, new TextEncoder().encode(c).length)}</code></li>`)
    .join('');
  return section(STRINGS.CFG.TOO_LONG_TITLE, STRINGS.CFG.TOO_LONG_ADVICE, rows, '#f44336');
}

function section(title, advice, rows, accent) {
  const style = accent ? ` style="color:${accent}"` : '';
  return `
      <strong${style}>${title}</strong>
      <ul style="margin:6px 0 6px 18px; padding:0;">${rows}</ul>
      <div style="opacity:.8">${advice}</div>`;
}

/**
 * Wraps each section from `section()` and separates them with a divider —
 * except the last, which gets none. Done here rather than via a CSS
 * `:last-child` rule because each wrapper needs its own inline
 * `border-bottom`, and an inline style always beats a stylesheet selector
 * regardless of specificity, so a CSS-only "remove it on the last one"
 * rule can never actually take effect.
 */
function joinSections(parts) {
  return parts
    .map((html, i) => {
      const style =
        i === parts.length - 1
          ? ''
          : ' style="margin-bottom:16px; padding-bottom:16px; border-bottom:1px solid rgba(255,255,255,.12);"';
      return `<div${style}>${html}</div>`;
    })
    .join('');
}

/**
 * Fills one of the three per-field banners and shows/hides it independently
 * of the other two — each field's warnings sit directly under that field
 * (same reasoning as `.path-warning` in Path Routing) rather than all piling
 * into one banner under Initial Commands regardless of which field they're
 * actually about.
 */
function renderInto(elId, sectionsHtml) {
  const el = document.querySelector(elId);
  if (!el) return;

  if (!sectionsHtml) {
    el.hidden = true;
    el.innerHTML = '';
    return;
  }

  el.hidden = false;
  el.style.cssText =
    'margin: 0 0 8px; padding: 10px 12px; border: 1px solid #b58900; ' +
    'border-radius: 4px; background: #2a2410; color: #e8dcb0; font-size: 12px;';
  el.innerHTML = sectionsHtml;
}

function render() {
  const unseen = report?.unseen ?? [];
  const conflicts = report?.conflicts ?? [];
  const asymmetric = report?.asymmetric ?? [];
  const custom = report?.custom ?? [];
  const bannedInit = report?.bannedInit ?? [];
  const bannedScheduled = report?.bannedScheduled ?? [];
  const tooLongInit = report?.tooLongInit ?? [];
  const tooLongScheduled = report?.tooLongScheduled ?? [];
  const decalDefaultRing = report?.decalDefaultRing ?? null;
  const decalFlushIsNoop = report?.decalFlushIsNoop ?? false;
  const noopInit = report?.noopInit ?? [];
  const noopScheduled = report?.noopScheduled ?? [];
  const fatalCvars = report?.fatalCvars ?? [];
  const configCfgWritable = report?.configCfgWritable ?? false;
  // Banned commands are already flagged, more specifically, in the banned
  // section above — MID_DEMO_HAZARDS is a superset of BANNED_COMMANDS on the
  // Rust side, so without this a banned command would otherwise also show up
  // here as a mere "breaks the decal flush" hazard, understating a command
  // that's actually going to block Start Capture Batch outright.
  const bannedScheduledCvars = new Set(bannedScheduled.map((b) => cvarOf(b.command)));
  const hazards = custom.filter((c) => c.kind === 'hazard' && !bannedScheduledCvars.has(cvarOf(c.command)));
  // Each conflict shows once: under Scheduled Commands when a Before is part
  // of it (that one is the last word), otherwise under Initial Commands.
  const initConflicts = conflicts.filter((c) => !c.scheduled);
  const scheduledConflicts = conflicts.filter((c) => c.scheduled);

  // ── Initial Commands ─────────────────────────────────────────────────────
  // Game Config (unseen) belongs here, not its own block: the fix it advises
  // is "state this in Initial Commands", so that is where seeing it is useful.
  const initParts = [];
  // First of all, and ahead of even the block-on-Start-Capture-Batch case
  // below: a config already sitting on disk will quit the game outright the
  // moment the HUD renders, whether or not this batch ever starts. Nothing
  // here can fix a file the app does not write to, so it is not blocking --
  // but it is the most severe fact this banner can report.
  if (fatalCvars.length > 0) {
    const rows = fatalCvars
      .map((f) => `<li><code>${STRINGS.CFG.fatalRow(f.cvar, f.value, f.required, f.file, f.line)}</code></li>`)
      .join('');
    initParts.push(section(STRINGS.CFG.FATAL_TITLE, STRINGS.CFG.FATAL_ADVICE, rows, '#f44336'));
  }
  // Next — this one blocks Start Capture Batch, everything else below it is
  // merely advisory.
  if (bannedInit.length > 0) {
    const rows = bannedInit
      .map((b) => `<li><code>${STRINGS.CFG.bannedRowDetailed(b.command, STRINGS.CFG.BANNED_REASONS[cvarOf(b.command)])}</code></li>`)
      .join('');
    initParts.push(section(STRINGS.CFG.BANNED_TITLE, STRINGS.CFG.BANNED_ADVICE, rows, '#f44336'));
  }
  if (tooLongInit.length > 0) {
    initParts.push(tooLongSection(tooLongInit));
  }
  // Next, because a value somebody stated is being thrown away.
  if (initConflicts.length > 0) {
    initParts.push(conflictSection(initConflicts));
  }
  if (unseen.length > 0) {
    const rows = unseen
      .map(
        (f) =>
          `<li><code>${f.cvar} ${f.value}</code> — ${STRINGS.CFG.location(f.file, f.line)}</li>`
      )
      .join('');
    initParts.push(section(STRINGS.CFG.BANNER_TITLE, STRINGS.CFG.ADVICE, rows));
  }
  // Real, active problem — not a neutral FYI like decalDefaultRing below —
  // so it gets a warning accent even though there is only ever one row.
  if (decalFlushIsNoop) {
    const rows = `<li><code>${STRINGS.CFG.DECAL_NOOP_ROW}</code></li>`;
    initParts.push(section(STRINGS.CFG.DECAL_NOOP_TITLE, STRINGS.CFG.DECAL_NOOP_ADVICE, rows, '#f44336'));
  }
  // Last, and no accent: nothing is wrong with taking the default, so this
  // is an FYI rather than something to fix, unlike everything above it.
  if (decalDefaultRing != null) {
    const rows = `<li><code>${STRINGS.CFG.decalDefaultRow(decalDefaultRing)}</code></li>`;
    initParts.push(section(STRINGS.CFG.DECAL_DEFAULT_TITLE, STRINGS.CFG.DECAL_DEFAULT_ADVICE, rows));
  }
  // Weakest signal of all — not wrong, not silently changing behavior, just
  // wasted keystrokes — so it goes last and gets no accent, same as the
  // default-ring FYI above.
  if (noopInit.length > 0) {
    const rows = noopInit
      .map((n) => `<li><code>${STRINGS.CFG.noopRow(n.command, STRINGS.CFG.NOOP_REASONS[n.cvar], n.source)}</code></li>`)
      .join('');
    initParts.push(section(STRINGS.CFG.NOOP_TITLE, STRINGS.CFG.NOOP_ADVICE, rows));
  }
  // Advisory, like the no-op list: nothing is wrong with this capture, but
  // its values leak into the user's own config the next time the game quits.
  if (configCfgWritable) {
    const rows = `<li>${STRINGS.CFG.CONFIG_WRITABLE_ROW}</li>`;
    initParts.push(section(STRINGS.CFG.CONFIG_WRITABLE_TITLE, STRINGS.CFG.CONFIG_WRITABLE_ADVICE, rows));
  }
  renderInto('#init-commands-warning-banner', joinSections(initParts));

  // ── Scheduled Commands ───────────────────────────────────────────────────
  const schedParts = [];
  // First of all — this one blocks Start Capture Batch.
  if (bannedScheduled.length > 0) {
    const rows = bannedScheduled
      .map((b) => `<li><code>${STRINGS.CFG.bannedRowDetailed(b.command, STRINGS.CFG.BANNED_REASONS[cvarOf(b.command)])}</code></li>`)
      .join('');
    schedParts.push(section(STRINGS.CFG.BANNED_TITLE, STRINGS.CFG.BANNED_ADVICE, rows, '#f44336'));
  }
  if (tooLongScheduled.length > 0) {
    schedParts.push(tooLongSection(tooLongScheduled));
  }
  // Next, because this one does not merely surprise: it breaks the
  // flush and leaves a capture that completes and looks plausible.
  if (hazards.length > 0) {
    const rows = hazards
      .map((h) => `<li><code>${STRINGS.CFG.hazardRow(h.command)}</code></li>`)
      .join('');
    schedParts.push(section(STRINGS.CFG.HAZARD_TITLE, STRINGS.CFG.HAZARD_ADVICE, rows, '#ff6b6b'));
  }
  // Next: a value that silently differs from the first clip on.
  if (asymmetric.length > 0) {
    const rows = asymmetric
      .map((a) => {
        const text = STRINGS.CFG.asymmetricRow(
          a.cvar,
          a.baseline.value,
          describeSource(a.baseline, a.cvar),
          a.after.value,
          describeSource(a.after, a.cvar)
        );
        return `<li><code>${text}</code></li>`;
      })
      .join('');
    schedParts.push(section(STRINGS.CFG.ASYMMETRIC_TITLE, STRINGS.CFG.ASYMMETRIC_ADVICE, rows, '#ff8a5c'));
  }
  if (scheduledConflicts.length > 0) {
    schedParts.push(conflictSection(scheduledConflicts));
  }
  if (noopScheduled.length > 0) {
    const rows = noopScheduled
      .map((n) => `<li><code>${STRINGS.CFG.noopRow(n.command, STRINGS.CFG.NOOP_REASONS[n.cvar], n.source)}</code></li>`)
      .join('');
    schedParts.push(section(STRINGS.CFG.NOOP_TITLE, STRINGS.CFG.NOOP_ADVICE, rows));
  }
  renderInto('#scheduled-commands-warning-banner', joinSections(schedParts));
}
