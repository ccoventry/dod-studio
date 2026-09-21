import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { checkForUpdate, downloadAndInstallUpdate, restartApp, getAppVersion, isDebugBuild } from './ipc_bridge.js';
import { notify } from './os_notifications.js';
import { STRINGS } from './strings.js';

function currentChannel() {
  return document.querySelector('#config-update-channel')?.value || 'stable';
}

function setStatus(text) {
  const el = document.querySelector('#update-status-text');
  if (el) el.textContent = text;
}

// Reflected on the Help menu's Check for Updates item (#update-check-btn,
// moved from the footer into #help-menu-panel by #122) plus a small dot on
// the Help menu button itself (#help-menu-update-dot) — the menu item alone
// isn't visible until Help is opened, and an update being ready is exactly
// the kind of thing that should be noticeable without opening the menu, the
// same ambient-signal role the old always-visible footer button served.
function setFooterButtonState(updateAvailable) {
  const btn = document.querySelector('#update-check-btn');
  const dot = document.querySelector('#help-menu-update-dot');
  if (dot) dot.style.display = updateAvailable ? '' : 'none';
  if (!btn) return;
  if (updateAvailable) {
    btn.textContent = STRINGS.FOOTER.UPDATE_AVAILABLE_BUTTON;
    btn.title = STRINGS.FOOTER.UPDATE_AVAILABLE_TITLE;
    btn.style.color = '#5fb85f';
    btn.style.borderColor = '#5fb85f';
  } else {
    btn.textContent = STRINGS.FOOTER.CHECK_UPDATES_BUTTON;
    btn.title = STRINGS.FOOTER.CHECK_UPDATES_TITLE;
    btn.style.color = '';
    btn.style.borderColor = '';
  }
}

// A local dev-server session or a debug-profile bundle never carries a real
// channel version, so any comparison against a published manifest reads as
// "different" essentially always — for a local session in particular,
// that's actively backwards (it's typically running the newest code,
// ahead of anything published), not just a noisy false positive.
export async function isLocalOrDebugBuild() {
  return import.meta.env.DEV || (await isDebugBuild());
}

// Fetched once at startup rather than re-read per update check — the
// running app's own version never changes without a restart, so there's
// nothing to keep re-fetching. This is also why the OS window title set
// below only ever changes across a relaunch (e.g. after an update installs)
// — flipping the Update Channel dropdown alone changes only which channel
// the *next* check polls, not what build is currently running.
async function displayCurrentVersion() {
  const version = await getAppVersion();
  if (!version) return;
  // Experimental channel versions carry a `-<run number>` suffix (see
  // release_experimental.yml); a stable release never has a `-` at all. But
  // two other cases read identically to that check alone: `npm run tauri dev`
  // (import.meta.env.DEV, true only under the Vite dev server) and
  // `tauri build --debug` (a real bundled install, just not a release-
  // profile binary — cfg!(debug_assertions), read via is_debug_build()).
  const [baseVersion, suffix] = version.split(/-(.+)/);
  let buildKind;
  if (import.meta.env.DEV) {
    buildKind = 'local';
  } else if (await isDebugBuild()) {
    buildKind = 'debug';
  } else {
    buildKind = suffix ? 'experimental' : 'stable';
  }
  // OS window title (taskbar/Alt-Tab), not an in-page element — used to be
  // a footer label (#122) before moving here.
  getCurrentWindow().setTitle(STRINGS.NAV.appWindowTitle(baseVersion, buildKind)).catch((err) => {
    console.error('Failed to set window title:', err);
  });
  const modalLabel = document.querySelector('#update-modal-current-version');
  if (modalLabel) modalLabel.textContent = STRINGS.UPDATE_MODAL.currentVersionLabel(version);
  const aboutLabel = document.querySelector('#about-modal-version');
  if (aboutLabel) aboutLabel.textContent = STRINGS.UPDATE_MODAL.currentVersionLabel(version);
}

export async function checkForUpdatesNow(channel = currentChannel()) {
  setStatus(STRINGS.UPDATE_MODAL.STATUS_CHECKING);
  const downloadBtn = document.querySelector('#download-install-update-btn');

  let info;
  try {
    info = await checkForUpdate(channel);
  } catch (err) {
    // ipc_bridge.js already toasted the failure — this just reflects it
    // into the modal's inline status line too.
    setStatus(STRINGS.UPDATE_MODAL.STATUS_CHECK_FAILED(err));
    return;
  }

  if (info) {
    setStatus(STRINGS.UPDATE_MODAL.STATUS_AVAILABLE(info.version));
    if (downloadBtn) downloadBtn.style.display = 'inline-block';
    setFooterButtonState(true);
    notify(
      'updates',
      STRINGS.FOOTER.UPDATE_AVAILABLE_BUTTON,
      STRINGS.UPDATE_MODAL.STATUS_AVAILABLE(info.version),
    );
  } else {
    setStatus(STRINGS.UPDATE_MODAL.STATUS_UP_TO_DATE);
    if (downloadBtn) downloadBtn.style.display = 'none';
    setFooterButtonState(false);
  }
}

async function beginDownloadAndInstall() {
  const downloadBtn = document.querySelector('#download-install-update-btn');
  const progressContainer = document.querySelector('#update-progress-container');
  setStatus(STRINGS.UPDATE_MODAL.STATUS_DOWNLOADING);
  if (downloadBtn) downloadBtn.disabled = true;
  if (progressContainer) progressContainer.style.display = 'block';
  try {
    await downloadAndInstallUpdate();
  } catch {
    // ipc_bridge.js already toasted; leave the button re-enabled so the user
    // can retry rather than getting stuck on a dead "Download & Install".
    if (downloadBtn) downloadBtn.disabled = false;
  }
}

/** Wires the footer's Check for Updates button and the standalone update
 *  modal's controls, listens for download progress/completion, and —
 *  unless `settings.auto_check_updates` is off, or this is a local/debug
 *  build — runs one background check on startup against
 *  `settings.update_channel`. Called once from main.js's DOMContentLoaded,
 *  after settings have loaded (not awaited — fire-and-forget).
 *
 *  `persistSettings` is main.js's persistAppSettings — every other settings
 *  control in the app calls it on change (it's also what refreshes
 *  os_notifications.js's live enabledFlags, per its own comment), but these
 *  three never had a listener wired, so toggling them did nothing until
 *  whatever next unrelated action happened to trigger a save. */
export async function initUpdater(settings, persistSettings) {
  const modal = document.querySelector('#update-modal');
  displayCurrentVersion();

  document.querySelector('#update-check-btn')?.addEventListener('click', () => {
    if (modal) modal.style.display = 'flex';
  });

  document.querySelector('#update-modal-close-btn')?.addEventListener('click', () => {
    if (modal) modal.style.display = 'none';
  });

  for (const id of ['#config-update-channel', '#config-auto-check-updates', '#config-notify-updates']) {
    document.querySelector(id)?.addEventListener('change', () => persistSettings?.());
  }

  document.querySelector('#check-updates-now-btn')
    ?.addEventListener('click', () => checkForUpdatesNow());

  document.querySelector('#download-install-update-btn')
    ?.addEventListener('click', () => beginDownloadAndInstall());

  document.querySelector('#restart-to-apply-btn')?.addEventListener('click', async () => {
    // app.restart() is a hard process restart — it never goes through the
    // window's onCloseRequested handler (main.js), which is the only other
    // place settings get saved. Without this, anything changed since the
    // last real save (not just in this modal) is silently lost the moment
    // the app relaunches.
    await persistSettings?.();
    restartApp();
  });

  listen('update_download_progress', (event) => {
    const { downloaded, total } = event.payload || {};
    const bar = document.querySelector('#update-progress-bar');
    if (bar && total) {
      bar.max = total;
      bar.value = downloaded;
    }
  });

  listen('update_ready', () => {
    setStatus(STRINGS.UPDATE_MODAL.STATUS_READY);
    const downloadBtn = document.querySelector('#download-install-update-btn');
    if (downloadBtn) downloadBtn.style.display = 'none';
    const restartBtn = document.querySelector('#restart-to-apply-btn');
    if (restartBtn) restartBtn.style.display = 'inline-block';
    notify(
      'updates',
      STRINGS.UPDATE_MODAL.RESTART_BUTTON,
      STRINGS.UPDATE_MODAL.STATUS_READY,
    );
  });

  if (settings?.auto_check_updates === false) return;
  if (await isLocalOrDebugBuild()) return;
  checkForUpdatesNow(settings?.update_channel || 'stable');
}
