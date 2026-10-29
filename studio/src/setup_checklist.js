// setup_checklist.js
// What DoD Studio needs before it can capture, and how much of it is set
// (#599). Pure: the Setup window (setup_window.js) and the Highlights page's
// checklist card (setup_card.js) both read one answer from here, so they
// never disagree about what is done. No DOM, no IPC.

/** The steps in the order the Setup window walks them. */
export const SETUP_STEPS = ['hl', 'hlae', 'ffmpeg', 'captures', 'renders', 'demos'];

/** The steps Start Capture Batch needs; the others are optional. */
export const REQUIRED_STEPS = ['hl', 'hlae', 'captures'];

/**
 * Whether `hlPath` is the hl.exe in Steam's own `steamapps/common/Half-Life`
 * folder -- normally the copy people play online with, which must never have
 * HLAE or our hook DLL loaded into it (docs/vac_safety.md, #373). A warning,
 * not a block: someone may keep only that one install and never play online.
 */
export function isSteamPlayInstall(hlPath) {
  const normalised = (hlPath || '').trim().replace(/\\/g, '/').toLowerCase();
  return normalised.endsWith('/steamapps/common/half-life/hl.exe');
}

/**
 * Whether Studio would launch Steam's own Half-Life with its own `dod`
 * folder, the pair people play online with. A copy of the dod folder beside
 * it (`dod_movies`, picked as the Game Folder) keeps movie files out of that
 * game, so only this pair gets the "highly recommended" note.
 */
export function usesPlayFolder(hlPath, gameFolder) {
  return isSteamPlayInstall(hlPath) && (gameFolder || 'dod').trim().toLowerCase() === 'dod';
}

/** The program each path must name: any other .exe (hlds.exe sits beside
 *  hl.exe in every install) passes the picker's filter but can't run. */
export const EXE_NAMES = { hl: 'hl.exe', hlae: 'HLAE.exe' };

/** The file name of `path` when it is not `expected` (case doesn't matter),
 *  else ''. A blank path is not wrong, only unset. */
export function wrongExeName(path, expected) {
  const name = (path || '').trim().split(/[\\/]/).pop();
  return name && name.toLowerCase() !== expected.toLowerCase() ? name : '';
}

/** A path state from diagnose_executable_paths counts as set only when it
 *  names a file, and the right one. An unknown state (no check ran yet)
 *  trusts the text. */
function pathIsSet(path, state, expected) {
  if (!(path || '').trim() || wrongExeName(path, expected)) return false;
  return state == null || state === 'ok';
}

/** A folder list counts once a folder is in it. Free space, when known,
 *  must be above zero on at least one entry: a list of unmounted drives is
 *  not a place captures can go. */
function foldersAreSet(folders) {
  const list = (folders || []).filter((f) => f && (f.path || '').trim());
  if (!list.length) return false;
  const measured = list.filter((f) => typeof f.freeBytes === 'number');
  return !measured.length || measured.some((f) => f.freeBytes > 0);
}

/**
 * Which steps are done.
 *
 * @param {object} state
 *   hlPath, hlaePath: the two executables as typed.
 *   hlState, hlaeState: 'ok' | 'empty' | 'not_found' | 'not_a_file' | undefined.
 *   ffmpegUsable: whether HLAE can run FFmpeg (null while unknown).
 *   appFfmpeg: the ffmpeg.exe DoD Studio itself renders with, '' when it
 *     finds none (undefined while unknown).
 *   targetDrives, renderExportDirs: [{ path, freeBytes }].
 *   demoCount: demos in the Master Queue.
 *   pinnedFolders: the remembered demo folders.
 * @returns {{ steps: Array<{key, required, done}>, doneCount, total,
 *   requiredDone, requiredTotal, allRequiredDone, nextIndex }}
 *   `nextIndex` is the first step not done (the required ones first), or 0.
 */
export function setupChecklist(state = {}) {
  const done = {
    hl: pathIsSet(state.hlPath, state.hlState, EXE_NAMES.hl),
    hlae: pathIsSet(state.hlaePath, state.hlaeState, EXE_NAMES.hlae),
    // Both halves: HLAE for Video capture, DoD Studio for Render and Finish
    // clips. HLAE's own bundled copy leaves Studio without one.
    ffmpeg: state.ffmpegUsable === true && state.appFfmpeg !== '',
    captures: foldersAreSet(state.targetDrives),
    renders: foldersAreSet(state.renderExportDirs),
    demos: (state.demoCount || 0) > 0 || (state.pinnedFolders || []).length > 0,
  };
  const steps = SETUP_STEPS.map((key) => ({
    key,
    required: REQUIRED_STEPS.includes(key),
    done: done[key],
  }));
  const required = steps.filter((s) => s.required);
  const requiredDone = required.filter((s) => s.done).length;
  const firstRequiredTodo = steps.findIndex((s) => s.required && !s.done);
  const firstTodo = steps.findIndex((s) => !s.done);
  return {
    steps,
    doneCount: steps.filter((s) => s.done).length,
    total: steps.length,
    requiredDone,
    requiredTotal: required.length,
    allRequiredDone: requiredDone === required.length,
    nextIndex: firstRequiredTodo !== -1 ? firstRequiredTodo : (firstTodo !== -1 ? firstTodo : 0),
  };
}

/**
 * Whether the Setup window opens by itself at launch: something required is
 * missing and the user has not skipped setup. A fresh install has every path
 * blank, so this covers the first launch with no flag of its own; a path that
 * goes missing later (a moved install) reopens it, because finishing setup
 * clears the skip (setup_window.js).
 */
export function shouldOpenSetupAtLaunch(checklist, setupDismissed) {
  return !checklist.allRequiredDone && !setupDismissed;
}

/** The index of `key` in the window's step order, 0 for an unknown key. */
export function stepIndex(key) {
  const i = SETUP_STEPS.indexOf(key);
  return i === -1 ? 0 : i;
}

/** "D:" for a drive path, "\\server\share" for a UNC path, else ''. */
export function driveLabel(path) {
  const p = (path || '').trim();
  const drive = p.match(/^([A-Za-z]):/);
  if (drive) return `${drive[1].toUpperCase()}:`;
  const unc = p.match(/^[\\/]{2}([^\\/]+)[\\/]([^\\/]+)/);
  if (unc) return `\\\\${unc[1]}\\${unc[2]}`;
  return '';
}

/** Bytes as "412 GB" or "1.6 TB"; under a gigabyte in MB. */
export function formatBytes(bytes) {
  if (typeof bytes !== 'number' || !(bytes >= 0)) return '';
  const tb = bytes / 1024 ** 4;
  if (tb >= 1) return `${tb < 10 ? tb.toFixed(1) : Math.round(tb)} TB`;
  const gb = bytes / 1024 ** 3;
  if (gb >= 1) return `${gb < 10 ? gb.toFixed(1) : Math.round(gb)} GB`;
  return `${Math.round(bytes / 1024 ** 2)} MB`;
}

/** The movie install's demo folder: `dod` beside hl.exe. '' with no path. */
export function defaultDemoFolder(hlPath) {
  const p = (hlPath || '').trim();
  const cut = Math.max(p.lastIndexOf('\\'), p.lastIndexOf('/'));
  if (cut <= 0) return '';
  const sep = p[cut];
  return `${p.slice(0, cut)}${sep}dod`;
}

/** Folders after the first on a drive, each with the earlier folder on that
 *  drive: `[{ path, drive, first }]`. Folders on one drive share its free
 *  space, so the later one adds no room (Render still scans capture folders
 *  for takes, so it's a warning, not a refusal). */
export function sameDriveFolders(paths) {
  const firstOn = new Map();
  const repeats = [];
  for (const raw of paths || []) {
    const path = (raw || '').trim();
    const drive = driveLabel(path);
    if (!drive) continue;
    const key = drive.toLowerCase();
    if (firstOn.has(key)) repeats.push({ path, drive, first: firstOn.get(key) });
    else firstOn.set(key, path);
  }
  return repeats;
}

/** The engine build behind a packet-entity limit (packet_entity_limit.js):
 *  1024 is the 25th Anniversary build, 256 the one before it. */
export function engineBuild(limit) {
  if (limit === 1024) return 'anniversary';
  if (limit === 256) return 'pre_anniversary';
  return null;
}
