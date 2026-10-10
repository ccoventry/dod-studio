// project_session.js
// The project file: Save / Load / New Session, the unsaved-changes prompt and
// the session indicator in the header. Moved out of main.js (#683), which
// owns the queue, the take index and the Teams list; they're reassigned
// there, so they're reached through getters and setters rather than held.

import { open, save } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { defaultProjectsDir } from './ipc_bridge.js';
import { updateStreakVisuals } from './detail_pane.js';
import { clearCheckedPaths } from './master_pane.js';
import { switchNavTab } from './nav.js';
import { streakUid, pruneTakeIndex } from './take_index.js';
import { emptyProjectTeams, normalizeProjectTeams } from './project_teams.js';
import { refreshTeamsPane } from './teams_pane.js';
import { projectFolders } from './project_paths.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

/**
 * @param {object} app  What this needs from main.js:
 *   getDemos() the queue, replaceDemos(demos) main.js's replaceScannedDemos,
 *   loadDemos(demos) make a loaded project's demos the queue and show them,
 *   getTakeIndex()/setTakeIndex(index), getProjectTeams()/setProjectTeams(teams),
 *   and checkMissingDemos(projectPath, scanPaths) from project_demos.js.
 */
export function createProjectSession({
  getDemos,
  replaceDemos,
  loadDemos,
  getTakeIndex,
  setTakeIndex,
  getProjectTeams,
  setProjectTeams,
  checkMissingDemos,
}) {
  // The project session file last loaded or saved in this window, if any —
  // once set, "Save Session" writes straight back to it instead of asking
  // Save-As every time (matches Ctrl+S's behavior in every other app).
  let currentSessionPath = null;
  // True whenever project state (scanned demos, takeIndex, scanPaths) has
  // changed since the last successful save or load — gates the "unsaved
  // changes" prompt on window close. Cleared by saveProjectSession() and
  // Load Session; set by markProjectDirty() at every mutation site.
  let hasUnsavedChanges = false;

  /** Every highlight's durable uid across every currently-scanned demo — the
   *  "still exists" set pruneTakeIndex() checks the take index against on save. */
  function collectAllUids() {
    const uids = [];
    getDemos().forEach(demo => {
      (demo.streaks || []).forEach(streak => uids.push(streakUid(demo.path, streak)));
    });
    return uids;
  }

  // Whether saveProjectSession() would actually have something to write: a
  // non-empty queue is always savable, but so is an emptied one once a
  // session file exists to write it back to — clearing everything is a
  // real, meaningful change relative to that file, not a no-op.
  function hasSavableProject() {
    return getDemos().length > 0 || !!currentSessionPath;
  }

  // Whether closing, reloading, loading or starting a new session should ask
  // first. Gated on hasSavableProject() too, matching saveProjectSession()'s
  // own guard — without it, clearing a *fresh, never-saved* queue to empty
  // then closing would show the prompt but "Save & Close" would just hit the
  // "Nothing to save" toast and leave the modal stuck open. Emptying a queue
  // that *did* come from a loaded session is still real, savable work
  // (writes the now-empty project back), so that case still prompts.
  function needsSavePrompt() {
    return hasUnsavedChanges && hasSavableProject();
  }

  function updateSessionFileIndicator() {
    const el = document.querySelector('#session-file-indicator');
    if (!el) return;
    const dirtySuffix = hasUnsavedChanges ? ' • unsaved' : '';
    if (currentSessionPath) {
      const filename = currentSessionPath.split(/[\\/]/).pop() || currentSessionPath;
      el.textContent = filename + dirtySuffix;
      el.title = currentSessionPath;
    } else {
      el.textContent = STRINGS.NAV.NO_SESSION_LOADED + dirtySuffix;
      el.title = '';
    }
  }

  // Marks Capture Studio's project state as changed since the last save —
  // called at every mutation site for currentScannedDemos/takeIndex/
  // scanPaths. Gates the close-window "unsaved changes" prompt.
  function markProjectDirty() {
    hasUnsavedChanges = true;
    updateSessionFileIndicator();
  }

  // Save Project Session — also called from the Clear All modal's "Save
  // Session First" action, so it lives here as a plain function rather than
  // only inline in the button's click handler. Returns whether it actually
  // wrote a file (false on "nothing to save" or a cancelled Save-As dialog).
  async function saveProjectSession() {
    if (!hasSavableProject()) {
      showToast(STRINGS.MAIN.NOTHING_TO_SAVE, 'info');
      return false;
    }
    // Already matches what's on disk — skip the write and the misleading
    // "saved" toast, but still report success so callers that gate on the
    // return value (Clear All's Save-First, the close-window prompt) treat
    // this the same as an actual save rather than a failure.
    if (!hasUnsavedChanges) {
      showToast(STRINGS.MAIN.ALREADY_SAVED, 'info');
      return true;
    }
    try {
      // Once a session's been loaded or saved once in this window, keep
      // writing back to that same file instead of asking Save-As again.
      const projectsDir = currentSessionPath ? null : await defaultProjectsDir();
      const filePath = currentSessionPath || await save({
        title: STRINGS.MAIN.SAVE_PROJECT_SESSION_TITLE,
        defaultPath: projectsDir ? `${projectsDir}\\dod_project.json` : 'dod_project.json',
        filters: [{ name: STRINGS.MAIN.JSON_PROJECT_FILTER_NAME, extensions: ['json'] }]
      });
      if (!filePath) return false;

      const hlaePath = document.querySelector('#hlae-path-input')?.value || "";
      const hlPath = document.querySelector('#hl-path-input')?.value || "";
      const projectData = JSON.stringify({
        version: "0.12.0",
        // The folders this project's demos are in, not the app-wide pinned
        // list (that is every folder ever added, nothing to do with the
        // project). Read back only to look for demos that have moved (#21).
        scanPaths: projectFolders(getDemos()),
        demos: getDemos(),
        hlaePath: hlaePath,
        hlPath: hlPath,
        // Pruned against what's actually still scanned so the index
        // doesn't accumulate uids for demos removed from the project.
        takeIndex: pruneTakeIndex(getTakeIndex(), collectAllUids()),
        teams: getProjectTeams(),
        // Kept for older-file/older-version compatibility — nothing on the
        // reading side branches on it any more (Quick-Clip mode is gone).
        mode: 'workspace'
      }, null, 2);
      await invoke('save_project_session', { path: filePath, contents: projectData });
      currentSessionPath = filePath;
      hasUnsavedChanges = false;
      updateSessionFileIndicator();
      showToast(STRINGS.MAIN.projectSavedToast(filePath), 'success');
      return true;
    } catch (err) {
      console.error("Save project error:", err);
      showToast(STRINGS.MAIN.SAVE_PROJECT_ERROR, 'error');
      return false;
    }
  }

  document.querySelector('#save-project-btn')?.addEventListener('click', () => saveProjectSession());

  // Load Project Session
  document.querySelector('#load-project-btn')?.addEventListener('click', async () => {
    // Loading replaces currentScannedDemos/takeIndex wholesale — same
    // data-loss risk as closing the window, so it gets the same prompt
    // before that happens, reusing the same modal.
    if (needsSavePrompt()) {
      const outcome = await requestUnsavedChangesConfirmation();
      if (!outcome) return; // Cancel — abort the load, keep current state
      // 'save' already wrote the file inside the modal's Save button
      // handler; 'discard' falls through to load over it either way.
    }
    try {
      const projectsDir = await defaultProjectsDir();
      const selected = await open({
        multiple: false,
        ...(projectsDir ? { defaultPath: projectsDir } : {}),
        filters: [{ name: STRINGS.MAIN.JSON_PROJECT_FILTER_NAME, extensions: ['json'] }]
      });
      if (selected) {
        const content = await invoke('load_project_session', { path: selected });
        const data = JSON.parse(content);
        if (data) {
          currentSessionPath = selected;
          hasUnsavedChanges = false;
          updateSessionFileIndicator();
          // Load Session is reachable from any tab (#122) — jump to Studio
          // so the loaded project is actually visible, same cross-tab-jump
          // pattern as detail_pane.js's "View Match Telemetry" button.
          switchNavTab('workspace');
          clearCheckedPaths();
          if (data.hlaePath) {
            const hlaeInput = document.querySelector('#hlae-path-input');
            if (hlaeInput) hlaeInput.value = data.hlaePath;
          }
          if (data.hlPath) {
            const hlInput = document.querySelector('#hl-path-input');
            if (hlInput) hlInput.value = data.hlPath;
          }
          // Tolerant: a 0.10.0 project file has no takeIndex at all — load
          // as empty rather than reject the file. Auto-Rendered just won't
          // retroactively apply to takes captured before this existed.
          const takeIndex = data.takeIndex || {};
          setTakeIndex(takeIndex);
          // Deliberately verbose: this is the only place takeIndex is ever
          // populated from disk, so logging it here — with exactly what
          // came out of the file, before anything else touches it — is
          // what makes it possible to prove a later auto-Rendered flip
          // came from this loaded data and not a leftover in-memory state.
          console.log(`[take-index] Loaded from ${selected}: ${Object.keys(takeIndex).length} take(s)`, takeIndex);
          // Tolerant the same way: a project saved before the Teams list
          // (#445) has no `teams`, and loads with none named or merged.
          setProjectTeams(normalizeProjectTeams(data.teams));
          if (data.demos) {
            // timeline_string is a derived field, saved as a convenience
            // snapshot rather than the source of truth — recompute it from
            // each streak's raw kills on every load so a display-only fix
            // (e.g. a weapon-name-resolution bug) shows correctly for
            // sessions saved before the fix, instead of replaying whatever
            // text got baked in at save time.
            data.demos.forEach(demo => (demo.streaks || []).forEach(updateStreakVisuals));
            loadDemos(data.demos);
            showToast(STRINGS.MAIN.loadedDemosToast(data.demos.length), 'success');
            await checkMissingDemos(selected, data.scanPaths || []);
          }
          refreshTeamsPane();
        }
      }
    } catch (err) {
      console.error("Load project error:", err);
      showToast(STRINGS.MAIN.LOAD_PROJECT_ERROR, 'error');
    }
  });

  // New Session (#122/#149) — resets to the same blank state the app starts
  // in: no session file, no demos, no take index. Reuses main.js's
  // replaceScannedDemos for the demo-queue reset — same as Clear All — then
  // overrides the dirty flag it sets, since a brand new untitled session has
  // nothing to prompt about saving.
  async function newSession() {
    if (needsSavePrompt()) {
      const outcome = await requestUnsavedChangesConfirmation();
      if (!outcome) return; // Cancel — abort, keep current state
    }
    replaceDemos([]);
    currentSessionPath = null;
    setTakeIndex({});
    setProjectTeams(emptyProjectTeams());
    refreshTeamsPane();
    hasUnsavedChanges = false;
    updateSessionFileIndicator();
    switchNavTab('workspace');
    showToast(STRINGS.MAIN.NEW_SESSION_TOAST, 'success');
  }

  document.querySelector('#new-session-btn')?.addEventListener('click', () => newSession());

  // Unsaved-changes prompt, shown by the window close handler whenever
  // hasUnsavedChanges is set. Same Promise-resolution shape as main.js's
  // tracked-work modal, but its own two-way branch ('save'/'discard') since
  // closing is a binary "keep the work or don't" rather than a "confirm a
  // removal."
  let pendingUnsavedChangesResolve = null;
  const unsavedChangesModal = document.querySelector('#unsaved-changes-modal');

  function requestUnsavedChangesConfirmation() {
    if (unsavedChangesModal) unsavedChangesModal.style.display = 'flex';
    return new Promise(resolve => { pendingUnsavedChangesResolve = resolve; });
  }

  if (unsavedChangesModal) {
    document.querySelector('#unsaved-changes-cancel-btn')?.addEventListener('click', () => {
      unsavedChangesModal.style.display = 'none';
      pendingUnsavedChangesResolve?.(false);
      pendingUnsavedChangesResolve = null;
    });
    document.querySelector('#unsaved-changes-discard-btn')?.addEventListener('click', () => {
      unsavedChangesModal.style.display = 'none';
      pendingUnsavedChangesResolve?.('discard');
      pendingUnsavedChangesResolve = null;
    });
    document.querySelector('#unsaved-changes-save-btn')?.addEventListener('click', async () => {
      const saved = await saveProjectSession();
      if (!saved) return; // Save-As cancelled/failed — leave the modal open
      unsavedChangesModal.style.display = 'none';
      pendingUnsavedChangesResolve?.('save');
      pendingUnsavedChangesResolve = null;
    });
  }

  return { markProjectDirty, saveProjectSession, needsSavePrompt, requestUnsavedChangesConfirmation };
}
