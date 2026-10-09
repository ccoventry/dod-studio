import { switchNavTab } from './nav.js';
import { openAnalyzerDemo } from './analyzer_pane.js';
import { launchDemoPreview, generateAllPreviews, checkEngineProcesses, killEngineProcesses, sendPreviewToRunningGame } from './ipc_bridge.js';
import { showToast } from './toast.js';
import { ensureSteamReady } from './steam_guard.js';
import { isRangeModified as isKillRangeModified, setStatusByHand, restoreStatus, isSkipped, setCuration, CURATION } from './take_index.js';
import { STRINGS } from './strings.js';
import { automaticClipName } from './clip_name.js';
import { getClipNameTemplate, refreshClipNamePreview } from './clip_name_ui.js';
import { confirmOverLimit } from './packet_entity_limit.js';
import { highlightStartSeconds, highlightDurationSeconds, formatClock } from './highlight_time.js';
import { refreshAfterTyping } from './input_refresh.js';
import { statusColor as colorOfStatus } from './status_colors.js';
import { escapeHtml } from './html.js';

let currentDemo = null;
let currentDemoIdx = null;
// Getter supplied by main.js so "Generate All Previews" can aggregate selected
// streaks across every loaded demo, not just the one currently displayed.
let currentGetAllDemos = null;
// Optional callback supplied by main.js — re-runs capture_pane.js's disk
// space launch guard, since toggling a streak's selection changes the
// required-bytes side of that comparison. Fired on every renderDetailView()
// call, including pure re-renders (selecting a different demo, a capture/
// render finishing) — NOT a signal that something was edited. See
// currentOnDirty below for that.
let currentOnSelectionChange = null;
// Optional callback supplied by main.js — marks the project dirty. Fired
// only from the actual field-edit listeners below (checkbox, kill range,
// status, notes), never from a plain re-render, so loading a session or
// switching the selected demo doesn't falsely flag unsaved changes.
let currentOnDirty = null;

export function initDetailPane(getAllDemos, onSelectionChange, onDirty) {
  currentGetAllDemos = getAllDemos;
  currentOnSelectionChange = onSelectionChange || null;
  currentOnDirty = onDirty || null;
}

// ── Running Process Guard (Half-Life Preview Detector) ────────────────────────
// launch_demo_preview patches and immediately launches HLAE; doing that while
// a prior hl.exe/hlae.exe is still alive corrupts the new session. When
// checkEngineProcesses() reports a conflict, the launch is parked here until
// the user resolves it via the modal (Force Relaunch / Copy View Command /
// Cancel) instead of proceeding blind.
//
// `pendingLaunch.run` is a generic no-arg callback so this one shared modal
// can park intents from other panes too (e.g. capture_pane.js's standalone
// "Launch Game (HLAE)" button) without each caller needing its own
// duplicate set of click listeners on the same modal buttons.
let pendingLaunch = null;

function demoStemFromPath(path) {
  const base = String(path || '').split(/[\\/]/).pop() || '';
  return base.replace(/\.dem$/i, '');
}

function showProcessDetectorModal() {
  const modal = document.querySelector('#process-detector-modal');
  if (modal) modal.style.display = 'flex';
}

function hideProcessDetectorModal() {
  const modal = document.querySelector('#process-detector-modal');
  if (modal) modal.style.display = 'none';
}

/** Parks a launch intent behind the Half-Life Preview Detector modal —
 *  `runFn` is invoked (no args) once the user picks "Force Relaunch" and any
 *  prior hl.exe/hlae.exe instance has been killed. Exported so other panes
 *  can reuse the same guarded-launch flow instead of duplicating it.
 *
 *  Pass `{ forBatch: true }` for a capture batch, which fails for a different
 *  reason and has nothing to copy a view command for. */
export function requestProcessGuardedLaunch(runFn, options = {}) {
  pendingLaunch = { run: runFn };
  applyProcessModalCopy(options.forBatch === true);
  showProcessDetectorModal();
}

// The modal is shared, so its wording has to match whichever intent parked the
// launch. A preview corrupts a session quietly; a batch is refused outright by
// the engine, and only after every demo has already been patched. "Copy View
// Command" has nothing to copy in the batch case.
function applyProcessModalCopy(forBatch) {
  const parts = [
    ['#process-modal-title', forBatch ? 'TITLE_BATCH' : 'TITLE'],
    ['#process-modal-body', forBatch ? 'BODY_BATCH' : 'BODY'],
  ];
  for (const [selector, key] of parts) {
    const el = document.querySelector(selector);
    if (!el) continue;
    // Both the attribute and the text: the text is what is on screen now, the
    // attribute is what a later language switch re-reads.
    el.dataset.str = `PROCESS_DETECTOR_MODAL.${key}`;
    el.textContent = STRINGS.PROCESS_DETECTOR_MODAL[key];
  }
  const copyBtn = document.querySelector('#process-modal-copy-command-btn');
  if (copyBtn) copyBtn.style.display = forBatch ? 'none' : '';
}

/** Reflects current demo-load state onto the Launch Preview (per-demo) and
 *  Generate All Previews (global) buttons — disabled only when there is no
 *  demo to act on at all. Neither button is gated on highlight selection
 *  (see #128): clicking with nothing checked now reaches the backend and
 *  surfaces its own "No highlights selected to preview." error as a toast,
 *  rather than silently no-op'ing with zero feedback. */
function updatePreviewButtonStates() {
  const launchBtn = document.querySelector('#btn-launch-preview');
  if (launchBtn) {
    launchBtn.disabled = !currentDemo;
  }
  const generateAllBtn = document.querySelector('#btn-generate-all-previews');
  if (generateAllBtn) {
    const allDemos = currentGetAllDemos ? currentGetAllDemos() : [];
    generateAllBtn.disabled = (allDemos || []).length === 0;
  }
  if (currentOnSelectionChange) currentOnSelectionChange();
}

/**
 * Recomputes start_tick/end_tick/kill_count/duration_string/timeline_string
 * from streak.kills[start_index..=end_index]. Mirrors
 * `CaptureStreak::update_visuals` in native/src/patch/types.rs — must be
 * called after any mutation of start_index/end_index so the display and the
 * eventual capture payload (which is built from these same fields) agree.
 */
export function updateStreakVisuals(streak) {
  if (!streak.kills || streak.kills.length === 0) return;

  // A session saved before start_index/end_index existed (or a streak never
  // touched by the range editor) may have either as undefined.
  if (streak.end_index === undefined) streak.end_index = streak.kills.length - 1;
  if (streak.start_index === undefined) streak.start_index = 0;

  const end = Math.min(streak.end_index, streak.kills.length - 1);
  const start = Math.min(streak.start_index, end);
  streak.start_index = start;
  streak.end_index = end;

  const slice = streak.kills.slice(start, end + 1);
  streak.start_tick = slice[0][0];
  streak.end_tick = slice[slice.length - 1][0];
  streak.kill_count = slice.length;

  const totalSecs = Math.round(Math.max(slice[slice.length - 1][1] - slice[0][1], 0));
  streak.duration_string = `${Math.floor(totalSecs / 60)}:${String(totalSecs % 60).padStart(2, '0')}`;

  const parts = slice.map(([, absTime, weapon], i) => {
    // Falls back to a labelled placeholder rather than an empty string so an
    // unresolved weapon name (e.g. a missing localization key) can never
    // leave a blank array element — `Array.prototype.join` would otherwise
    // render that as an orphaned leading/embedded separator with no name.
    const weaponClean = String(weapon || '').replace(/^Weapon::/, '').trim() || STRINGS.ANALYZER.WEAPON_UNKNOWN;
    if (i === 0) return weaponClean;
    const gapSec = Math.round(Math.max(absTime - slice[i - 1][1], 0));
    return `(+${Math.floor(gapSec / 60)}:${String(gapSec % 60).padStart(2, '0')}) ${weaponClean}`;
  });
  streak.timeline_string = parts.join(', ');
}

/** Selects every currently-visible (POV + Min Kills filtered) streak in
 *  `currentDemo` — the header checkbox's checked state. Only the rows on
 *  screen. `demo.streaks` holds every player's streaks, and the table shows
 *  the recording player's at or above Min Kills — so selecting the raw list
 *  checked highlights belonging to other players that were never rendered
 *  and could not be unchecked. The batch groups by (demo, target player), so
 *  each of those became its own chained demo: one 15-row table produced five
 *  passes over the same file. */
function selectAllVisibleStreaks() {
  if (!currentDemo || !currentDemo.streaks) return;
  currentDemo.streaks.forEach(s => {
    // A Skip row stays unticked: it's locked out of every batch (#44).
    if (isVisibleStreak(currentDemo, s) && !isSkipped(s)) s.selected = true;
  });
  renderDetailView(currentDemo, currentDemoIdx);
}

/** Deselects every streak in `currentDemo` — the header checkbox's
 *  unchecked state. Deliberately NOT filtered, unlike selectAllVisibleStreaks.
 *  This is the escape hatch: if anything ever selects a streak the table does
 *  not show, this is what clears it. Erring wide costs nothing here; erring
 *  narrow leaves a capture running that nobody asked for. */
function deselectAllStreaks() {
  if (!currentDemo || !currentDemo.streaks) return;
  currentDemo.streaks.forEach(s => { s.selected = false; });
  renderDetailView(currentDemo, currentDemoIdx);
}

// Initialize event listeners for detail pane buttons
window.addEventListener("DOMContentLoaded", () => {
  const inputMinKills = document.querySelector('#input-min-kills');
  const btnLaunchPreview = document.querySelector('#btn-launch-preview');
  const btnGenerateAllPreviews = document.querySelector('#btn-generate-all-previews');

  if (inputMinKills) {
    inputMinKills.addEventListener('input', () => {
      renderDetailView(currentDemo, currentDemoIdx);
    });
  }

  /** Actually invokes launch_demo_preview — shared by the direct click path
   *  (no conflicting process) and the modal's Force Relaunch path. */
  async function performLaunchPreview(hlaePath, hlPath, highlights) {
    btnLaunchPreview.disabled = true;
    const originalLabel = btnLaunchPreview.textContent;
    btnLaunchPreview.textContent = STRINGS.HIGHLIGHTS.LAUNCHING;
    const goldsrcHooksDllPath = document.querySelector('#goldsrc-hooks-dll-path-input')?.value?.trim() || null;
    try {
      await launchDemoPreview(hlaePath, hlPath, highlights, goldsrcHooksDllPath);
      showToast(STRINGS.HIGHLIGHTS.PREVIEW_LAUNCHING_TOAST, 'info');
    } catch (err) {
      // Already toasted by ipc_bridge.js.
    } finally {
      btnLaunchPreview.textContent = originalLabel;
      updatePreviewButtonStates();
    }
  }

  // The game is already open: if DoD Studio started it, its hook DLL takes
  // console commands, so the preview goes straight to it (#413). Resolves true
  // when it did; false leaves the caller to show the "already running" prompt.
  async function sendPreviewToOpenGame(hlaePath, hlPath, highlights) {
    btnLaunchPreview.disabled = true;
    const originalLabel = btnLaunchPreview.textContent;
    btnLaunchPreview.textContent = STRINGS.HIGHLIGHTS.LAUNCHING;
    const goldsrcHooksDllPath = document.querySelector('#goldsrc-hooks-dll-path-input')?.value?.trim() || null;
    try {
      const sent = await sendPreviewToRunningGame(hlaePath, hlPath, highlights, goldsrcHooksDllPath);
      if (sent) {
        showToast(STRINGS.HIGHLIGHTS.sentToRunningGame(sent), 'success');
        return true;
      }
      return false;
    } catch (err) {
      // Already toasted by ipc_bridge.js; the prompt still offers a way on.
      return false;
    } finally {
      btnLaunchPreview.textContent = originalLabel;
      updatePreviewButtonStates();
    }
  }

  if (btnLaunchPreview) {
    btnLaunchPreview.addEventListener('click', async () => {
      const hlaePath = document.querySelector('#hlae-path-input')?.value?.trim();
      const hlPath = document.querySelector('#hl-path-input')?.value?.trim();
      if (!hlaePath || !hlPath) {
        showToast(STRINGS.HIGHLIGHTS.HLAE_PATH_REQUIRED, 'error');
        return;
      }
      if (!currentDemo || !currentDemo.streaks) return;
      const highlights = allPreviewableStreaks(currentDemo);
      // The game closes on a demo over its entity limit (#207).
      if (!(await confirmOverLimit([currentDemo], hlPath, { preview: true }))) return;

      let engineAlreadyRunning = false;
      try {
        engineAlreadyRunning = await checkEngineProcesses();
      } catch (err) {
        // Already toasted by ipc_bridge.js — fail open rather than blocking
        // a legitimate launch just because the detector itself errored.
      }

      if (engineAlreadyRunning) {
        if (await sendPreviewToOpenGame(hlaePath, hlPath, highlights)) return;
        requestProcessGuardedLaunch(() => performLaunchPreview(hlaePath, hlPath, highlights));
        return;
      }

      if (!(await ensureSteamReady())) return;
      await performLaunchPreview(hlaePath, hlPath, highlights);
    });
  }

  const processModalForceRelaunchBtn = document.querySelector('#process-modal-force-relaunch-btn');
  if (processModalForceRelaunchBtn) {
    processModalForceRelaunchBtn.addEventListener('click', async () => {
      if (!pendingLaunch) {
        hideProcessDetectorModal();
        return;
      }
      const { run } = pendingLaunch;
      processModalForceRelaunchBtn.disabled = true;
      try {
        await killEngineProcesses();
      } catch (err) {
        // Already toasted by ipc_bridge.js.
      }
      await new Promise(resolve => setTimeout(resolve, 500));
      processModalForceRelaunchBtn.disabled = false;
      pendingLaunch = null;
      hideProcessDetectorModal();
      await run();
    });
  }

  const processModalCopyCommandBtn = document.querySelector('#process-modal-copy-command-btn');
  if (processModalCopyCommandBtn) {
    processModalCopyCommandBtn.addEventListener('click', async () => {
      const stem = demoStemFromPath(currentDemo?.path);
      const viewCommand = `viewdemo ${stem}_preview`;
      try {
        await navigator.clipboard.writeText(viewCommand);
        showToast(STRINGS.HIGHLIGHTS.copiedViewCommand(viewCommand), 'success');
      } catch (err) {
        console.error("Failed to copy view command to clipboard:", err);
        showToast(STRINGS.HIGHLIGHTS.COPY_VIEW_COMMAND_FAILED, 'error');
      }
      hideProcessDetectorModal();
    });
  }

  const processModalCancelBtn = document.querySelector('#process-modal-cancel-btn');
  if (processModalCancelBtn) {
    processModalCancelBtn.addEventListener('click', () => {
      pendingLaunch = null;
      hideProcessDetectorModal();
    });
  }

  if (btnGenerateAllPreviews) {
    btnGenerateAllPreviews.addEventListener('click', async () => {
      const hlaePath = document.querySelector('#hlae-path-input')?.value?.trim();
      const hlPath = document.querySelector('#hl-path-input')?.value?.trim();
      if (!hlaePath || !hlPath) {
        showToast(STRINGS.HIGHLIGHTS.HLAE_PATH_REQUIRED, 'error');
        return;
      }
      const allDemos = currentGetAllDemos ? currentGetAllDemos() : [];
      const allHighlights = (allDemos || []).flatMap(d => allPreviewableStreaks(d));

      btnGenerateAllPreviews.disabled = true;
      const originalLabel = btnGenerateAllPreviews.textContent;
      btnGenerateAllPreviews.textContent = STRINGS.HIGHLIGHTS.GENERATING;
      try {
        const count = await generateAllPreviews(hlaePath, hlPath, allHighlights);
        showToast(STRINGS.HIGHLIGHTS.generatedPreviews(count), 'success');
      } catch (err) {
        // Already toasted by ipc_bridge.js.
      } finally {
        btnGenerateAllPreviews.textContent = originalLabel;
        updatePreviewButtonStates();
      }
    });
  }
});

/**
 * Whether a streak is one of the rows the Highlights table actually shows.
 *
 * Two filters, and both matter to anything that acts on "all" of them:
 *
 *  1. **The recording player only.** `demo.streaks` holds every player's
 *     streaks — everyone in the server. Gated on `local_player_index` rather
 *     than `demo.is_pov`, because is_pov fires on any demo containing
 *     SvcHltv/SvcDirector messages, which a normal player recording also picks
 *     up when an HLTV caster happens to be spectating. `scan_demo_for_highlights`
 *     already rejects true HLTV proxy files, so a missing index means "no
 *     resolvable owner", not "spectator demo".
 *  2. **Min Kills.**
 *
 * Select All uses this too. When it did not, it checked highlights belonging to
 * other players that were never rendered and could not be unchecked — and since
 * the batch builder groups by (demo, target player), each of those became its
 * own chained demo. One fifteen-row table turned into five passes over the same
 * file.
 */
function isVisibleStreak(demo, streak, minKills) {
  if (!demo || !streak) return false;
  if (demo.local_player_index !== null && demo.local_player_index !== undefined) {
    if (streak.player_index !== demo.local_player_index) return false;
  }
  const threshold = minKills ?? parseInt(document.querySelector('#input-min-kills')?.value || "1", 10);
  return streak.kill_count >= threshold;
}

/** Every one of `demo`'s highlights for the recording player, regardless of
 *  Min Kills or checkbox selection (minKills 0 bypasses that half of
 *  isVisibleStreak's filter, keeping only the player filter). Previewing is
 *  how you decide what's worth checking in the first place, not a view of
 *  what's already checked — the bookmark-preview patch has nothing to do
 *  with either filter (see #128). */
function allPreviewableStreaks(demo) {
  if (!demo || !demo.streaks) return [];
  return demo.streaks.filter(s => isVisibleStreak(demo, s, 0));
}

export function renderDetailView(demo, selectedDemoIdx) {
  currentDemo = demo;
  currentDemoIdx = selectedDemoIdx;
  updatePreviewButtonStates();

  const titleEl = document.querySelector('#detail-demo-title');
  const container = document.querySelector('#detail-streaks-container');
  const telemetryBtn = document.querySelector('#view-telemetry-btn');

  if (telemetryBtn) {
    if (!demo || !demo.path) {
      telemetryBtn.disabled = true;
      telemetryBtn.onclick = null;
    } else {
      telemetryBtn.disabled = false;
      telemetryBtn.onclick = () => {
        switchNavTab('demo-analyzer');
        openAnalyzerDemo(demo.path);
      };
    }
  }

  if (!titleEl || !container) return;

  if (!demo) {
    // No "(Select a Demo)" here — the empty-state message below already
    // says exactly that; repeating it in the title was pure redundancy.
    titleEl.textContent = STRINGS.HIGHLIGHTS.DEFAULT_TITLE;
    titleEl.title = '';
    container.innerHTML = `<p style="color: #888;">${STRINGS.HIGHLIGHTS.EMPTY_SELECT_DEMO}</p>`;
    return;
  }

  const minKills = parseInt(document.querySelector('#input-min-kills')?.value || "1", 10);
  titleEl.textContent = STRINGS.HIGHLIGHTS.detailTitle(demo.name);
  // CSS truncates this with an ellipsis at narrow widths (same treatment as
  // the Master Queue's demo-name column) — the title attribute keeps the
  // full name reachable on hover once it's cut off.
  titleEl.title = STRINGS.HIGHLIGHTS.detailTitle(demo.name);
  container.innerHTML = '';

  if (!demo.streaks || demo.streaks.length === 0) {
    container.innerHTML = `<p style="color: #888;">${STRINGS.HIGHLIGHTS.EMPTY_NO_STREAKS}</p>`;
    return;
  }

  const tableWrapper = document.createElement('div');
  tableWrapper.className = 'table-wrapper';
  const table = document.createElement('table');
  table.id = 'detail-streaks-table';
  table.innerHTML = `
    <thead>
      <tr>
        <th>${STRINGS.HIGHLIGHTS.COL_ROW_NUM}</th>
        <th>${STRINGS.HIGHLIGHTS.COL_SEL} <input type="checkbox" id="detail-select-all-cb" title="${STRINGS.HIGHLIGHTS.SELECT_ALL_CB_TITLE}" /></th>
        <th>${STRINGS.HIGHLIGHTS.COL_KILL_RANGE}</th>
        <th>${STRINGS.HIGHLIGHTS.COL_KILLS}</th>
        <th>${STRINGS.HIGHLIGHTS.COL_TIME}</th>
        <th>${STRINGS.HIGHLIGHTS.COL_DUR}</th>
        <th>${STRINGS.HIGHLIGHTS.COL_STATUS}</th>
        <th>${STRINGS.HIGHLIGHTS.COL_CLIP_NAME}</th>
        <th title="${STRINGS.HIGHLIGHTS.COL_REVIEW_TITLE}">${STRINGS.HIGHLIGHTS.COL_REVIEW}</th>
        <th class="col-notes">${STRINGS.HIGHLIGHTS.COL_NOTES}</th>
        <th>${STRINGS.HIGHLIGHTS.COL_DETAILS}</th>
      </tr>
    </thead>
    <tbody></tbody>
  `;
  const tbody = table.querySelector('tbody');

  // Sequential display numbering is tracked separately from the streak's
  // position in demo.streaks — POV/min-kills filtering below skips entries,
  // and Row # must count only rows actually rendered (matches dev's
  // `filtered_indices` + `row_idx + 1` behavior), not the raw array index.
  let renderedRowNum = 0;

  demo.streaks.forEach((streak, streakIdx) => {
    // Both filters live in isVisibleStreak so Select All applies exactly the
    // same rule. They disagreed once, and the result was five chained demos
    // from a fifteen-row table.
    if (!isVisibleStreak(demo, streak, minKills)) {
      return;
    }

    // Opt-In Default
    if (streak.selected === undefined) {
      streak.selected = false;
    }
    if (streak.start_index === undefined) streak.start_index = 0;
    if (streak.end_index === undefined) {
      streak.end_index = Math.max((streak.kills || []).length - 1, 0);
    }

    renderedRowNum++;
    const rowNum = renderedRowNum;

    const tr = document.createElement('tr');
    tr.style.borderBottom = '1px solid #333';
    // #44: a Skip row is dimmed, unticked and locked.
    const skipped = isSkipped(streak);
    if (skipped) {
      tr.style.opacity = '0.5';
      streak.selected = false;
    }

    // Time matches the demo player's clock; see highlight_time.js (#464).
    const durSecs = highlightDurationSeconds(streak, demo.tickrate).toFixed(1);
    const timeStr = formatClock(highlightStartSeconds(streak, demo.tickrate));

    // Details: precomputed weapon/timing chain from the backend
    // (e.g. "Rifle (+0:03) Rifle" — first kill weapon + gap + weapon chain).
    const timelineText = streak.timeline_string || STRINGS.HIGHLIGHTS.fallbackKillCount(streak.kill_count);

    // Shared with the Master Demo Queue's columns (status_colors.js, #527).
    const statusLabel = streak.status || STRINGS.HIGHLIGHTS.STATUS_UNSET_DEFAULT;
    const statusColor = colorOfStatus(statusLabel);

    const maxKillIdx = Math.max((streak.kills || []).length - 1, 0);
    const isRangeModified = isKillRangeModified(streak);

    // Set by capture_pane.js's capture_takes_verified handler when this
    // highlight's take was an overlap merge covering more than one highlight
    // (builder.rs's merge loop) — surfaced so it's obvious at a glance why
    // two rows flipped to Captured together instead of independently.
    const mergedBadge = streak.mergedTakeKey
      ? `<span title="${STRINGS.HIGHLIGHTS.mergedBadgeTitle(streak.mergedCount)}" style="margin-left:6px;font-size:0.75em;color:#ff9800;border:1px solid #ff9800;border-radius:2px;padding:1px 4px;cursor:help;">${STRINGS.HIGHLIGHTS.mergedTakeBadge(streak.mergedTakeKey.split('/').pop())}</span>`
      : '';

    const byHandMark = streak.statusByHand
      ? `<span class="status-by-hand-mark" title="${STRINGS.HIGHLIGHTS.STATUS_BY_HAND_TITLE}" style="margin-left:4px;color:#aaa;cursor:help;">${STRINGS.HIGHLIGHTS.STATUS_BY_HAND_MARK}</span>`
      : '';

    tr.innerHTML = `
      <td style="padding: 8px;">${rowNum}</td>
      <td style="padding: 8px;">
        <input type="checkbox" class="streak-select-cb" data-index="${streakIdx}" ${streak.selected ? 'checked' : ''} ${skipped ? `disabled title="${STRINGS.HIGHLIGHTS.SKIPPED_CB_TITLE}"` : ''} />
      </td>
      <td style="padding: 8px;">
        <div style="display:flex;align-items:center;gap:4px;${isRangeModified ? 'color:#ff9800;' : ''}">
          <input type="number" class="kr-start-input" min="1" max="${maxKillIdx + 1}"
                 value="${streak.start_index + 1}" style="width:38px;background:#1a1a1a;color:inherit;border:1px solid #444;border-radius:2px;" />
          <span>-</span>
          <input type="number" class="kr-end-input" min="1" max="${maxKillIdx + 1}"
                 value="${streak.end_index + 1}" style="width:38px;background:#1a1a1a;color:inherit;border:1px solid #444;border-radius:2px;" />
          ${isRangeModified ? `<button type="button" class="kr-reset-btn" title="${STRINGS.HIGHLIGHTS.KR_RESET_TITLE}" style="background:transparent;border:1px solid #555;border-radius:2px;color:#aaa;cursor:pointer;">↺</button>` : ''}
        </div>
      </td>
      <td style="padding: 8px; font-weight: bold;">${streak.kill_count}</td>
      <td style="padding: 8px;">${timeStr}</td>
      <td style="padding: 8px;">${STRINGS.HIGHLIGHTS.secondsSuffix(durSecs)}</td>
      <td style="padding: 8px;">
        <select class="streak-status-select" style="color: ${statusColor}; font-size: 0.85em;">
          ${STRINGS.HIGHLIGHTS.STATUS_OPTIONS.map(s =>
            // Each option in its own colour, not the selected one's (#527).
            `<option value="${s}" style="color: ${colorOfStatus(s)};" ${s === statusLabel ? 'selected' : ''}>${s}</option>`
          ).join('')}
        </select>${byHandMark}${mergedBadge}
      </td>
      <td style="padding: 8px;">
        <input type="text" class="streak-clip-name-input" spellcheck="false" title="${STRINGS.HIGHLIGHTS.CLIP_NAME_INPUT_TITLE}" style="background: #1a1a1a; color: #fff; border: 1px solid #444; border-radius: 3px; padding: 2px; width: 100%; min-width: 14em;" />
      </td>
      <td style="padding: 8px;">
        <select class="streak-curation-select" style="font-size: 0.85em;">
          <option value="" ${!streak.curation ? 'selected' : ''}>${STRINGS.HIGHLIGHTS.CURATION_UNREVIEWED}</option>
          <option value="${CURATION.KEEP}" ${streak.curation === CURATION.KEEP ? 'selected' : ''}>${STRINGS.HIGHLIGHTS.CURATION_KEEP}</option>
          <option value="${CURATION.SKIP}" ${skipped ? 'selected' : ''}>${STRINGS.HIGHLIGHTS.CURATION_SKIP}</option>
        </select>
      </td>
      <td style="padding: 8px;">
        <textarea class="streak-notes-input" rows="2" placeholder="${escapeHtml(STRINGS.HIGHLIGHTS.NOTES_PLACEHOLDER)}">${escapeHtml(streak.notes)}</textarea>
      </td>
      <td class="details-cell" title="${timelineText}">${timelineText}</td>
    `;

    const cb = tr.querySelector('.streak-select-cb');
    cb.addEventListener('change', (e) => {
      streak.selected = e.target.checked;
      updatePreviewButtonStates();
      if (currentOnSelectionChange) currentOnSelectionChange();
      if (currentOnDirty) currentOnDirty();
    });

    const startInput = tr.querySelector('.kr-start-input');
    const endInput = tr.querySelector('.kr-end-input');
    startInput.addEventListener('change', () => {
      const v = Math.min(Math.max(parseInt(startInput.value, 10) - 1, 0), streak.end_index);
      streak.start_index = Number.isNaN(v) ? 0 : v;
      updateStreakVisuals(streak);
      renderDetailView(currentDemo, currentDemoIdx);
      if (currentOnSelectionChange) currentOnSelectionChange();
      if (currentOnDirty) currentOnDirty();
    });
    endInput.addEventListener('change', () => {
      const v = Math.max(Math.min(parseInt(endInput.value, 10) - 1, maxKillIdx), streak.start_index);
      streak.end_index = Number.isNaN(v) ? maxKillIdx : v;
      updateStreakVisuals(streak);
      renderDetailView(currentDemo, currentDemoIdx);
      if (currentOnSelectionChange) currentOnSelectionChange();
      if (currentOnDirty) currentOnDirty();
    });

    const resetBtn = tr.querySelector('.kr-reset-btn');
    if (resetBtn) {
      resetBtn.addEventListener('click', () => {
        streak.start_index = 0;
        streak.end_index = maxKillIdx;
        updateStreakVisuals(streak);
        renderDetailView(currentDemo, currentDemoIdx);
        if (currentOnSelectionChange) currentOnSelectionChange();
        if (currentOnDirty) currentOnDirty();
      });
    }

    tr.querySelector('.streak-curation-select').addEventListener('change', (e) => {
      setCuration(streak, e.target.value);
      renderDetailView(currentDemo, currentDemoIdx);
      if (currentOnSelectionChange) currentOnSelectionChange();
      if (currentOnDirty) currentOnDirty();
    });

    const statusSelect = tr.querySelector('.streak-status-select');
    // Free in both directions (#105, D9); the mark and the Undo toast are
    // what keep a hand-set status honest.
    const afterStatusChange = () => {
      renderDetailView(currentDemo, currentDemoIdx);
      if (currentOnSelectionChange) currentOnSelectionChange();
      if (currentOnDirty) currentOnDirty();
    };
    statusSelect.addEventListener('change', (e) => {
      const previous = setStatusByHand(streak, e.target.value);
      afterStatusChange();
      showToast(STRINGS.HIGHLIGHTS.statusSetToast(e.target.value), 'info', 6000, {
        action: {
          label: STRINGS.HIGHLIGHTS.UNDO,
          onClick: () => {
            restoreStatus(streak, previous);
            afterStatusChange();
          },
        },
      });
    });

    // #441: the automatic name shows greyed until a typed one replaces it.
    const clipNameInput = tr.querySelector('.streak-clip-name-input');
    clipNameInput.value = streak.clipName || '';
    clipNameInput.placeholder = automaticClipName(demo, streak, getClipNameTemplate()).name;
    clipNameInput.addEventListener('input', (e) => {
      streak.clipName = e.target.value;
    });
    clipNameInput.addEventListener('change', (e) => {
      const typed = e.target.value.trim();
      if (typed) streak.clipName = typed;
      else delete streak.clipName;
      e.target.value = typed;
      refreshClipNamePreview();
      if (currentOnSelectionChange) currentOnSelectionChange();
      if (currentOnDirty) currentOnDirty();
    });

    const notesInput = tr.querySelector('.streak-notes-input');
    notesInput.addEventListener('input', (e) => {
      streak.notes = e.target.value;
    });
    // Master Queue's tracked badge (master_pane.js) depends on whether this
    // streak has a note. Refreshed shortly after typing stops, not per
    // keystroke, so typing doesn't rebuild the whole Master Queue table on
    // every character -- and not only on 'change', which an undo (Ctrl+Z)
    // never fires until blur, leaving the badge stale (#535).
    refreshAfterTyping(notesInput, () => {
      if (currentOnSelectionChange) currentOnSelectionChange();
      if (currentOnDirty) currentOnDirty();
    });

    tbody.appendChild(tr);
  });

  // Header checkbox tri-state, reflecting only the same visible rows the
  // table just rendered — mirrors master_pane.js's syncSelectAllCheckboxState
  // for the Master Queue's own header checkbox.
  const selectAllHeaderCb = table.querySelector('#detail-select-all-cb');
  if (selectAllHeaderCb) {
    // Skip rows can't be ticked, so they don't count toward "all ticked".
    const visibleStreaks = demo.streaks.filter(s => isVisibleStreak(demo, s, minKills) && !isSkipped(s));
    const selectedVisible = visibleStreaks.filter(s => s.selected).length;
    selectAllHeaderCb.checked = visibleStreaks.length > 0 && selectedVisible === visibleStreaks.length;
    selectAllHeaderCb.indeterminate = selectedVisible > 0 && selectedVisible < visibleStreaks.length;
    selectAllHeaderCb.addEventListener('change', (e) => {
      if (e.target.checked) selectAllVisibleStreaks();
      else deselectAllStreaks();
    });
  }

  tableWrapper.appendChild(table);
  container.appendChild(tableWrapper);
}

