// rescan_recent.js
// After an update that makes demos out of date, every recent session's
// outdated demos in one go (#938), instead of each session asking on its own
// open (project_rescan.js). Offered once per update, on the first start, and
// any time from File → Rescan Recent Sessions.
//
// Each session is backed up beside itself first
// (`<name>.before-rescan-<date>.json`), then opened, rescanned with the
// queue's own scan (queue_scan.js, which carries each highlight's work:
// rescan_carry.js) and saved, all through project_session.js, so the batch
// writes exactly what opening, rescanning and saving by hand would.

import { listen } from '@tauri-apps/api/event';
import { filesExist, readProjectFile, writeProjectFile, estimateRescan, getAppVersion, logFrontendEvent } from './ipc_bridge.js';
import { outdatedDemos, HIGHLIGHTS_VERSION } from './scan_version.js';
import { estimateSeconds, formatDuration, rescanTimeLine, getScanReadRate } from './rescan_estimate.js';
import { fileNameOf, folderOf } from './path_display.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

/** Over this many seconds in all, each session can be skipped for now. */
export const SKIP_OVER_SECS = 60;
/** The start-up offer needs this many sessions to rescan: one alone is
 *  covered by its own prompt when it opens. The File menu item takes one. */
export const MIN_SESSIONS_AT_STARTUP = 2;
/** Backups tried per session per day: `-2`, `-3`… after the first. */
const BACKUP_TRIES = 9;

// ── Which update the start-up offer last ran for (settings file) ────────────

let offeredFor = '';

/** The settings field, for settings_sync.js's save. */
export function rescanOfferSettings() {
  return { rescan_offered_for: offeredFor };
}

/** Takes it from a loaded settings file; an older file has none. */
export function applyRescanOfferSettings(settings) {
  const v = settings?.rescan_offered_for;
  offeredFor = typeof v === 'string' ? v : '';
}

/** What the offer is keyed on: the app's version, plus the highlight
 *  detection version so a build that bumps it offers again even when its
 *  version number didn't move. Null without a version. */
export function offerStamp(appVersion) {
  return appVersion ? `${appVersion}+h${HIGHLIGHTS_VERSION}` : null;
}

// ── Pure helpers ─────────────────────────────────────────────────────────────

/** "2026-10-23", in local time. */
export function localDate(d = new Date()) {
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** Where try `n` (1 first) of a session's backup goes: beside it, named
 *  `<name>.before-rescan-<date>.json`, then `…-<date>-2.json` and on, so a
 *  second rescan the same day keeps the first backup. */
export function backupPath(path, date, n = 1) {
  const name = fileNameOf(path);
  const stem = name.replace(/\.json$/i, '');
  const folder = name === path ? '' : folderOf(path);
  const sep = path.includes('\\') ? '\\' : '/';
  const file = `${stem}.before-rescan-${date}${n > 1 ? `-${n}` : ''}.json`;
  return folder ? `${folder}${sep}${file}` : file;
}

/** The first of `tries` that `taken` (one boolean per name, or null when
 *  the check failed) says is free. Null when none is, or when it can't tell:
 *  the session then fails rather than overwrite an earlier backup, which may
 *  be the only copy of how it was before any rescan. */
export function freeBackupPath(tries, taken) {
  if (!Array.isArray(taken)) return null;
  return tries.find((_, k) => taken[k] === false) ?? null;
}

/**
 * The offer's rows, one per recent session: `state` is 'outdated' (with
 * `demoPaths`, its outdated demos that are on disk), 'missing' (no file) or
 * 'unreadable'. A session with nothing to rescan has no row.
 * `exists(paths)` resolves one boolean per path, or null when it can't
 * tell; `read(path)` resolves the file's text.
 */
export async function planRecent(paths, { exists = filesExist, read = readProjectFile } = {}) {
  const found = await exists(paths);
  const rows = await Promise.all(paths.map(async (path, i) => {
    const row = { path, name: fileNameOf(path) };
    if (Array.isArray(found) && found[i] === false) return { ...row, state: 'missing' };
    try {
      const data = JSON.parse(await read(path));
      return { ...row, state: 'outdated', demoPaths: outdatedDemos(data?.demos).map((d) => d.path) };
    } catch (_) {
      return { ...row, state: 'unreadable' };
    }
  }));
  // One check for every session's demos: a demo that's gone can't be read.
  const all = rows.flatMap((r) => r.demoPaths || []);
  if (all.length > 0) {
    const onDisk = await exists(all);
    if (Array.isArray(onDisk)) {
      let k = 0;
      rows.forEach((r) => {
        if (!r.demoPaths) return;
        r.demoPaths = r.demoPaths.filter(() => onDisk[k++] !== false);
      });
    }
  }
  return rows.filter((r) => r.state !== 'outdated' || r.demoPaths.length > 0);
}

/** The rows a Rescan all works through. */
export function sessionsToRescan(rows) {
  return (rows || []).filter((r) => r.state === 'outdated' && !r.skip);
}

/** Whether the start-up offer shows for these rows. */
export function worthOfferingAtStartup(rows) {
  return (rows || []).filter((r) => r.state === 'outdated').length >= MIN_SESSIONS_AT_STARTUP;
}

/** Each session gets "Skip for now" only when the whole run is long. */
export function skipAllowed(totalSecs) {
  return Number(totalSecs) > SKIP_OVER_SECS;
}

/** How far the whole run is: session `i` (weighted by `weights`, its demo
 *  count plus one for the backup and save) is `frac` done. */
export function overallFraction(weights, i, frac) {
  const total = weights.reduce((a, b) => a + b, 0);
  if (!(total > 0)) return 0;
  const before = weights.slice(0, i).reduce((a, b) => a + b, 0);
  const f = Math.min(1, Math.max(0, Number(frac) || 0));
  return Math.min(1, (before + f * (weights[i] || 0)) / total);
}

const secs = (ms) => (Math.max(0, ms) / 1000).toFixed(1);

/**
 * Works through `rows`: logs the skipped and missing ones, then backs up,
 * opens, rescans and saves each session in turn. `shouldStop()` is asked
 * before each session (Cancel between sessions). A failed session is logged
 * and left; the next one still runs. Resolves `{ done, failed, total,
 * stopped }`.
 * @param {object} deps  backup(path) resolves the backup's path; open(path),
 *   scan(paths) and save() resolve whether they worked; currentOutdated()
 *   the open session's outdated demo paths; isOpen(path) whether that
 *   session is still the open one, asked before saving; exists(paths); log(line);
 *   onStep(i, n, row, what, frac); shouldStop(); now().
 */
export async function rescanSessions(rows, deps) {
  const S = STRINGS.RESCAN_RECENT;
  const { log, onStep = () => {}, shouldStop = () => false, now = Date.now, exists = filesExist, isOpen = () => true } = deps;
  (rows || []).forEach((r) => {
    if (r.state === 'missing') log(S.missingLog(r.path));
    else if (r.state === 'outdated' && r.skip) log(S.skippedLog(r.name));
  });
  const todo = sessionsToRescan(rows);
  const result = { done: 0, failed: 0, total: todo.length, stopped: false };
  for (let i = 0; i < todo.length; i++) {
    if (shouldStop()) {
      log(S.stoppedLog(todo.length - i));
      result.stopped = true;
      break;
    }
    const row = todo[i];
    const step = (what, frac) => onStep(i, todo.length, row, what, frac);
    const startedAt = now();
    try {
      step(S.STEP_BACKUP, 0);
      const backup = await deps.backup(row.path);
      step(S.STEP_OPEN, 0.05);
      if (!(await deps.open(row.path))) throw new Error(S.OPEN_FAILED);
      // From the session as it opened, not as it was counted: the file may
      // have been saved since (the unsaved-changes prompt's Save).
      let paths = deps.currentOutdated();
      const onDisk = paths.length > 0 ? await exists(paths) : [];
      if (Array.isArray(onDisk)) paths = paths.filter((_, k) => onDisk[k] !== false);
      if (paths.length === 0) {
        log(S.noDemosLog(row.name));
        result.done++;
        continue;
      }
      step(S.stepScan(paths.length), 0.05);
      if (!(await deps.scan(paths))) throw new Error(S.SCAN_FAILED);
      step(S.STEP_SAVE, 0.95);
      // Save writes whatever session is open: one opened or cleared while
      // this one scanned (Ctrl+O, Ctrl+N) would get its queue.
      if (!isOpen(row.path)) throw new Error(S.SESSION_CHANGED);
      if (!(await deps.save())) throw new Error(S.SAVE_FAILED);
      log(S.doneLog(row.name, paths.length, secs(now() - startedAt), fileNameOf(backup)));
      result.done++;
    } catch (err) {
      log(S.failedLog(row.name, secs(now() - startedAt), err?.message || err));
      result.failed++;
    }
  }
  return result;
}

// ── The dialog ──────────────────────────────────────────────────────────────

/**
 * @param {object} app  What this needs from main.js:
 *   getRecent() File → Open Recent's list (recent_projects.js);
 *   session, project_session.js's confirmReplaceProject(),
 *   loadProjectFile(path, opts), saveProjectSession(opts), newSession(opts)
 *   and currentPath(); getDemos() the queue; scan(paths) main.js's
 *   triggerAutoScan and isScanning() queue_scan.js's; isSetupOpen() the
 *   Setup window; persistAppSettings(), which saves the offer's stamp.
 *   The IPC calls and the clock are swappable for tests.
 */
export function createRescanRecent({
  getRecent,
  session,
  getDemos,
  scan,
  persistAppSettings,
  isScanning = () => false,
  isSetupOpen = () => false,
  exists = filesExist,
  read = readProjectFile,
  write = writeProjectFile,
  estimate = estimateRescan,
  appVersion = getAppVersion,
  log = logFrontendEvent,
  now = Date.now,
}) {
  const S = STRINGS.RESCAN_RECENT;
  const modal = document.querySelector('#rescan-recent-modal');
  const messageEl = document.querySelector('#rescan-recent-message');
  const listEl = document.querySelector('#rescan-recent-list');
  const totalEl = document.querySelector('#rescan-recent-total');
  const progressEl = document.querySelector('#rescan-recent-progress');
  const stepEl = document.querySelector('#rescan-recent-step');
  const fillEl = document.querySelector('#rescan-recent-progress .progress-bar-fill');
  const startBtn = document.querySelector('#rescan-recent-start-btn');
  const cancelBtn = document.querySelector('#rescan-recent-cancel-btn');

  // An offer or run is under way (one at a time); `running`: the run itself.
  let busy = false;
  let running = false;
  let stopRequested = false;
  let currentIndex = 0;
  // The session being rescanned's place in the bar, for scan_progress.
  let scanning = null;
  let answer = null;

  listen('scan_progress', (event) => {
    const p = event.payload || {};
    if (!scanning || !(p.total > 0)) return;
    setFraction(overallFraction(scanning.weights, scanning.i, 0.05 + (0.9 * (p.scanned || 0)) / p.total));
  }).catch(() => {});

  function setFraction(f) {
    if (fillEl) fillEl.style.width = `${Math.round(f * 1000) / 10}%`;
  }

  function show(shown) {
    if (modal) modal.style.display = shown ? 'flex' : 'none';
  }

  function renderRows(rows) {
    if (!listEl) return;
    listEl.replaceChildren();
    const timed = rows.filter((r) => r.state === 'outdated');
    const total = timed.reduce((sum, r) => sum + (r.skip ? 0 : r.secs || 0), 0);
    const allTimed = timed.every((r) => r.secs !== undefined);
    const canSkip = !running && allTimed && skipAllowed(timed.reduce((sum, r) => sum + (r.secs || 0), 0));
    rows.forEach((row) => {
      const li = document.createElement('li');
      li.className = 'rescan-recent-row';
      li.title = row.path;
      li.dataset.path = row.path;
      li.classList.toggle('skipped', !!row.skip);
      li.classList.toggle('unavailable', row.state !== 'outdated');
      const name = document.createElement('span');
      name.className = 'rescan-recent-name';
      name.textContent = row.name;
      const detail = document.createElement('span');
      detail.className = 'rescan-recent-detail';
      if (row.state === 'missing') detail.textContent = S.MISSING;
      else if (row.state === 'unreadable') detail.textContent = S.UNREADABLE;
      else detail.textContent = `${S.demos(row.demoPaths.length)} · ${row.timeText ?? S.TIME_PENDING}`;
      li.append(name, detail);
      if (row.state === 'outdated' && canSkip) {
        const skip = document.createElement('button');
        skip.type = 'button';
        skip.className = 'rescan-recent-skip';
        skip.textContent = row.skip ? S.SKIPPED : S.SKIP;
        skip.setAttribute('aria-pressed', row.skip ? 'true' : 'false');
        skip.addEventListener('click', () => {
          row.skip = !row.skip;
          renderRows(rows);
        });
        li.append(skip);
      }
      listEl.appendChild(li);
    });
    if (totalEl) {
      totalEl.textContent = [allTimed && timed.length > 0 ? S.total(formatDuration(total)) : '', canSkip ? S.SKIP_HINT : '']
        .filter(Boolean).join(' ');
    }
    if (startBtn) startBtn.disabled = running || sessionsToRescan(rows).length === 0;
  }

  // Each row's time (#937's estimate), filled in as they come.
  function estimateRows(rows) {
    const rate = getScanReadRate();
    rows.filter((r) => r.state === 'outdated').forEach((row) => {
      Promise.resolve()
        .then(() => estimate(row.demoPaths))
        .then((est) => {
          row.secs = estimateSeconds(est, rate);
          row.timeText = rescanTimeLine(est, rate);
        })
        .catch(() => {
          row.secs = 0;
          row.timeText = '';
        })
        .then(() => { if (!running) renderRows(rows); });
    });
  }

  function showReading() {
    if (messageEl) messageEl.textContent = S.READING;
    listEl?.replaceChildren();
    if (totalEl) totalEl.textContent = '';
    if (progressEl) progressEl.hidden = true;
    if (startBtn) {
      startBtn.hidden = false;
      startBtn.disabled = true;
    }
    if (cancelBtn) cancelBtn.disabled = true;
    show(true);
  }

  function askToStart(rows) {
    if (messageEl) messageEl.textContent = S.MESSAGE;
    if (progressEl) progressEl.hidden = true;
    if (startBtn) startBtn.hidden = false;
    if (cancelBtn) {
      cancelBtn.textContent = S.NOT_NOW;
      cancelBtn.disabled = false;
    }
    renderRows(rows);
    estimateRows(rows);
    show(true);
    startBtn?.focus();
    return new Promise((resolve) => { answer = resolve; });
  }

  startBtn?.addEventListener('click', () => {
    const resolve = answer;
    answer = null;
    resolve?.(true);
  });
  cancelBtn?.addEventListener('click', () => {
    if (answer) {
      const resolve = answer;
      answer = null;
      resolve(false);
      return;
    }
    if (running) {
      stopRequested = true;
      cancelBtn.disabled = true;
      if (stepEl) stepEl.textContent = `${stepEl.textContent.replace(/:.*$/, '')}: ${S.STOPPING}`;
    }
  });

  async function backup(path) {
    const contents = await read(path);
    const date = localDate(new Date(now()));
    const tries = Array.from({ length: BACKUP_TRIES }, (_, k) => backupPath(path, date, k + 1));
    const target = freeBackupPath(tries, await exists(tries));
    if (!target) throw new Error(S.NO_BACKUP_NAME);
    await write(target, contents);
    return target;
  }

  async function run(rows) {
    const todo = sessionsToRescan(rows);
    const weights = todo.map((r) => r.demoPaths.length + 1);
    const prior = session.currentPath();
    stopRequested = false;
    if (startBtn) startBtn.hidden = true;
    if (cancelBtn) {
      cancelBtn.textContent = S.CANCEL;
      cancelBtn.disabled = false;
    }
    if (progressEl) progressEl.hidden = false;
    setFraction(0);
    renderRows(rows);
    show(true);
    const result = await rescanSessions(rows, {
      backup,
      open: (path) => session.loadProjectFile(path, { quiet: true }),
      currentOutdated: () => outdatedDemos(getDemos()).map((d) => d.path),
      isOpen: (path) => session.currentPath() === path,
      exists,
      scan: async (paths) => {
        scanning = { weights, i: currentIndex };
        try {
          return await scan(paths);
        } finally {
          scanning = null;
        }
      },
      save: () => session.saveProjectSession({ quiet: true }),
      log,
      now,
      shouldStop: () => stopRequested,
      onStep: (i, n, row, what, frac) => {
        currentIndex = i;
        if (stepEl) stepEl.textContent = S.step(i + 1, n, row.name, stopRequested ? S.STOPPING : what);
        setFraction(overallFraction(weights, i, frac));
      },
    });
    setFraction(1);
    if (stepEl) stepEl.textContent = S.STEP_REOPEN;
    if (cancelBtn) cancelBtn.disabled = true;
    // Back to what was open before: that session as saved (rescanned, if it
    // was in the list), or the blank queue.
    if (prior) await session.loadProjectFile(prior, { rescanOffer: false });
    else await session.newSession({ quiet: true });
    showToast(S.doneToast(result.done, result.total, result.failed), result.failed > 0 ? 'warning' : 'success', 8000);
    return result;
  }

  /** The offer, then the run. `startup`: only when enough sessions need it,
   *  and silent when none do. Resolves the run's result, or null. */
  async function offer({ startup = false } = {}) {
    if (busy) {
      if (!startup) showToast(S.BUSY_TOAST, 'info');
      return null;
    }
    // A queue scan under way would merge its demos into whichever session
    // this opens, and that session would be saved with them.
    if (isScanning()) {
      if (!startup) showToast(S.SCAN_RUNNING_TOAST, 'info');
      return null;
    }
    const recent = getRecent() || [];
    if (recent.length === 0) {
      if (!startup) showToast(S.NONE_RECENT_TOAST, 'info');
      return null;
    }
    busy = true;
    try {
      // From the menu the dialog opens at once, saying what it's doing.
      if (!startup) showReading();
      const rows = await planRecent(recent, { exists, read });
      const wanted = startup ? worthOfferingAtStartup(rows) : sessionsToRescan(rows).length > 0;
      if (!wanted) {
        if (!startup) showToast(S.ALL_CURRENT_TOAST, 'info');
        return null;
      }
      const yes = await askToStart(rows);
      if (!yes) return null;
      // The open session is replaced: its unsaved changes are asked about
      // first, with the dialog out of the way.
      show(false);
      if (!(await session.confirmReplaceProject())) return null;
      // Asked again: one may have started while the dialogs were up.
      if (isScanning()) {
        showToast(S.SCAN_RUNNING_TOAST, 'info');
        return null;
      }
      running = true;
      return await run(rows);
    } finally {
      busy = false;
      running = false;
      scanning = null;
      show(false);
    }
  }

  /** The start-up check: once per update (offerStamp), with two or more
   *  recent sessions. The stamp is saved before asking, so Not now isn't
   *  asked again until the next update. Not over the Setup window or a
   *  running scan: then it waits for the next start, stamp unspent. */
  async function offerAfterUpdate() {
    if (isSetupOpen() || isScanning()) return null;
    const stamp = offerStamp(await appVersion());
    if (!stamp || stamp === offeredFor) return null;
    offeredFor = stamp;
    await persistAppSettings();
    if ((getRecent() || []).length < MIN_SESSIONS_AT_STARTUP) return null;
    return offer({ startup: true });
  }

  document.querySelector('#rescan-recent-btn')?.addEventListener('click', () => offer());

  return { offer, offerAfterUpdate };
}
