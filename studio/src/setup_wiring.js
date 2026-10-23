// setup_wiring.js
// Glue for the first-run Setup window and the Capture page's checklist card
// (#599): gathers what is set (paths, HLAE's FFmpeg, folders with their free
// space, demos) into one snapshot, redraws both from it, and runs their
// actions against the same fields and lists Configuration edits, so the two
// never hold separate copies of a setting.
import { pickPath } from './pick_path.js';
import { openUrl } from '@tauri-apps/plugin-opener';
import {
  calculateExportPoolSpace,
  checkHlaeFfmpeg,
  diagnoseExecutablePaths,
  enginePacketEntityLimit,
  gameFolderLanguageFile,
  listGameFolders,
  revealInExplorer,
} from './ipc_bridge.js';
import { getGameFolder, refreshGameFolderList } from './game_folder_ui.js';
import { createMovieCopy } from './movie_copy_ui.js';
import { refreshPathWarnings, refreshHlaeFfmpegStatus, linkHlaeToFfmpeg } from './config_wiring.js';
import { switchNavTab, setCaptureDetailSubtab } from './nav.js';
import { setupChecklist, shouldOpenSetupAtLaunch, engineBuild, stepIndex } from './setup_checklist.js';
import { renderSetupCard } from './setup_card.js';
import { prefillSingleInstall } from './install_picker.js';
import { createSetupWindow } from './setup_window.js';
import { STRINGS } from './strings.js';

const value = (sel) => document.querySelector(sel)?.value?.trim() || '';

/**
 * @param {object} deps
 *   getDemos, getScanPaths, getTargetDrives, getRenderExportDirs: main.js state.
 *   captureFolders, renderFolders: { add(path), remove(path) } on the two
 *     list editors, so Configuration's lists stay the one source.
 *   addDemoFolder(folder): queue_scan.js's scan-and-remember.
 *   persistAppSettings(): settings_sync.js.
 *   dismissed: the saved setup_dismissed setting.
 * @returns {{ refresh, start, isDismissed }}
 */
export function createSetup({
  getDemos,
  getScanPaths,
  getTargetDrives,
  getRenderExportDirs,
  captureFolders,
  renderFolders,
  addDemoFolder,
  persistAppSettings,
  dismissed = false,
}) {
  let setupDismissed = !!dismissed;
  let snapshot = {
    hlPath: '', hlaePath: '', targetDrives: [], renderExportDirs: [],
    demoCount: 0, pinnedFolders: [], ffmpegUsable: null,
  };
  // A slow refresh must not overwrite a newer one's answer.
  let generation = 0;

  async function freeSpace(paths) {
    return Promise.all((paths || []).filter(Boolean).map(async (path) => {
      try {
        return { path, freeBytes: await calculateExportPoolSpace([path]) };
      } catch {
        return { path };
      }
    }));
  }

  async function gather() {
    const hlPath = value('#hl-path-input');
    const hlaePath = value('#hlae-path-input');
    let states = [];
    try {
      states = await diagnoseExecutablePaths([hlPath, hlaePath]);
    } catch {
      // Logged by the bridge; an unknown state trusts the typed path.
    }
    const [hlState, hlaeState] = states;
    const hlBuild = hlPath && hlState === 'ok' ? engineBuild(await enginePacketEntityLimit(hlPath)) : null;
    let gameFolders = [];
    let gameFolderLanguage = null;
    if (hlPath && hlState === 'ok') {
      try {
        gameFolders = (await listGameFolders(hlPath)) || [];
      } catch {
        // Logged by the bridge; the step says to pick hl.exe first.
      }
      // A dod copy without its <folder>_<language>.txt (#925).
      gameFolderLanguage = await gameFolderLanguageFile(hlPath, getGameFolder());
    }
    let ffmpegUsable = null;
    let ffmpegCanLink = false;
    let appFfmpeg;
    let appFfmpegProblem = null;
    let hlaeBundledFfmpeg = null;
    if (hlaePath && hlaeState === 'ok') {
      try {
        const result = await checkHlaeFfmpeg(hlaePath, value('#ffmpeg-override-path-input') || 'ffmpeg');
        ffmpegUsable = !!result?.usable;
        ffmpegCanLink = !!result?.can_link;
        appFfmpeg = result?.app_ffmpeg || '';
        appFfmpegProblem = result?.app_ffmpeg ? (result?.app_ffmpeg_problem || null) : null;
        hlaeBundledFfmpeg = result?.state?.state === 'bundled' ? (result.state.path || null) : null;
      } catch {
        ffmpegUsable = false;
      }
    }
    const [targetDrives, renderExportDirs] = await Promise.all([
      freeSpace(getTargetDrives()),
      freeSpace(getRenderExportDirs()),
    ]);
    return {
      hlPath, hlState, hlaePath, hlaeState, hlBuild, ffmpegUsable, ffmpegCanLink,
      gameFolder: getGameFolder(), gameFolders, gameFolderLanguage,
      appFfmpeg, appFfmpegProblem, hlaeBundledFfmpeg,
      targetDrives, renderExportDirs,
      demoCount: (getDemos() || []).length,
      pinnedFolders: [...(getScanPaths() || [])],
    };
  }

  const card = document.querySelector('#setup-card');
  const tableWrapper = card?.parentElement?.querySelector('.table-wrapper');

  function draw() {
    const checklist = setupChecklist(snapshot);
    const showing = renderSetupCard(card, { checklist, snapshot }, { fix, open: openWindow });
    // The card stands in for the empty queue rather than sitting above it.
    if (tableWrapper) tableWrapper.style.display = showing && snapshot.demoCount === 0 ? 'none' : '';
    win.render();
    return checklist;
  }

  async function refresh() {
    const mine = ++generation;
    const next = await gather();
    if (mine !== generation) return setupChecklist(snapshot);
    snapshot = next;
    return draw();
  }

  // Configuration's own fields, so both Browse buttons share a start.
  const EXE_FIELDS = {
    '#hl-path-input': 'config.hl',
    '#hlae-path-input': 'config.hlae',
    '#ffmpeg-override-path-input': 'config.ffmpeg',
  };

  async function pickExe(inputSel, title) {
    let selected = null;
    try {
      selected = await pickPath({
        field: EXE_FIELDS[inputSel],
        current: document.querySelector(inputSel)?.value?.trim(),
        multiple: false,
        filters: [{ name: STRINGS.MAIN.EXECUTABLE_FILTER_NAME, extensions: ['exe'] }],
        title,
      });
    } catch (err) {
      console.error('Error opening the executable picker:', err);
    }
    await setExe(inputSel, Array.isArray(selected) ? selected[0] : selected);
  }

  async function setExe(inputSel, path) {
    if (!path) return;
    const input = document.querySelector(inputSel);
    if (input) {
      input.value = path;
      // The game path's other readers (the packet-entity limit, the config
      // warnings) listen for this.
      input.dispatchEvent(new Event('change'));
    }
    await persistAppSettings();
    await refreshPathWarnings();
    await refreshHlaeFfmpegStatus();
    await refresh();
  }

  async function pickFolder(field, title) {
    try {
      const selected = await pickPath({ field, directory: true, multiple: false, title });
      return (Array.isArray(selected) ? selected[0] : selected) || null;
    } catch (err) {
      console.error('Error opening the folder picker:', err);
      return null;
    }
  }

  const folders = {
    captures: { list: captureFolders, field: 'capture.output_folder', title: () => STRINGS.MAIN.SELECT_CAPTURE_OUTPUT_DIR_TITLE },
    renders: { list: renderFolders, field: 'render.export_folder', title: () => STRINGS.MAIN.SELECT_RENDER_EXPORT_DIR_TITLE },
  };

  async function addFolder(kind) {
    const path = await pickFolder(folders[kind].field, folders[kind].title());
    if (path) folders[kind].list.add(path);
    await refresh();
  }

  async function removeFolder(kind, path) {
    folders[kind].list.remove(path);
    await refresh();
  }

  async function pickAndScanDemos() {
    const folder = await pickFolder('queue.add_folder', STRINGS.MAIN.SELECT_DEMO_FOLDER_TITLE);
    if (folder) await addDemoFolder(folder);
    await refresh();
  }

  // Through Configuration's own Game Folder list, which saves it and runs
  // its checks.
  async function setGameFolder(name) {
    const select = document.querySelector('#config-game-folder');
    if (select && name) {
      select.value = name;
      select.dispatchEvent(new Event('change'));
    }
    await refreshPathWarnings();
    await refresh();
  }

  // Make a movie copy (#939): the new folder joins the Game Folder's list,
  // then is picked there.
  const movieCopy = createMovieCopy({
    getHlPath: () => value('#hl-path-input'),
    onDone: async (folder) => {
      await refreshGameFolderList();
      await setGameFolder(folder);
    },
  });

  const win = createSetupWindow({
    getSnapshot: () => snapshot,
    pickHl: () => pickExe('#hl-path-input', STRINGS.MAIN.SELECT_HL_EXE_TITLE),
    pickHlae: () => pickExe('#hlae-path-input', STRINGS.MAIN.SELECT_HLAE_EXE_TITLE),
    linkFfmpeg: async () => { await linkHlaeToFfmpeg(); await refresh(); },
    openHlaePage: () => openUrl(STRINGS.SETUP.HLAE_URL).catch((err) => console.error('Error opening the HLAE page:', err)),
    // Fills Configuration's FFmpeg Override Path, the one setting Render,
    // Finish clips and the HLAE link all read.
    pickFfmpeg: () => pickExe('#ffmpeg-override-path-input', STRINGS.MAIN.SELECT_FFMPEG_EXE_TITLE),
    useHlaeFfmpeg: () => setExe('#ffmpeg-override-path-input', snapshot.hlaeBundledFfmpeg),
    setGameFolder,
    openMovieCopy: () => movieCopy.open(),
    showLanguageFile: () => {
      const path = snapshot?.gameFolderLanguage?.show;
      if (path) revealInExplorer(path).catch(() => {});
    },
    openFfmpegPage: () => openUrl(STRINGS.SETUP.FFMPEG_URL).catch((err) => console.error('Error opening the FFmpeg page:', err)),
    addFolder,
    removeFolder,
    pickDemoFolder: () => pickFolder('queue.add_folder', STRINGS.MAIN.SELECT_DEMO_FOLDER_TITLE),
    onSkip: async () => {
      setupDismissed = true;
      await persistAppSettings();
    },
    onFinish: async (demoFolder) => {
      // Finishing re-arms the launch check, so a required path that goes
      // missing later brings the window back.
      setupDismissed = false;
      await persistAppSettings();
      switchNavTab('workspace');
      setCaptureDetailSubtab('highlights');
      if (demoFolder && !(getScanPaths() || []).includes(demoFolder)) await addDemoFolder(demoFolder);
      await refresh();
    },
  });

  function openWindow(step) {
    win.open(step);
    refresh();
  }

  function fix(key) {
    if (key === 'hl') pickExe('#hl-path-input', STRINGS.MAIN.SELECT_HL_EXE_TITLE);
    else if (key === 'hlae') pickExe('#hlae-path-input', STRINGS.MAIN.SELECT_HLAE_EXE_TITLE);
    else if (key === 'captures' || key === 'renders') addFolder(key);
    else if (key === 'demos') pickAndScanDemos();
    else openWindow(key);
  }

  // Configuration's own Browse buttons set these without a change event; the
  // window regaining focus after their picker closes covers those.
  for (const sel of ['#hl-path-input', '#hlae-path-input', '#ffmpeg-override-path-input']) {
    document.querySelector(sel)?.addEventListener('change', () => refresh());
  }
  window.addEventListener('focus', () => refresh());

  document.querySelector('#setup-menu-btn')?.addEventListener('click', async () => {
    const checklist = await refresh();
    openWindow(checklist.nextIndex);
  });

  /** First draw at launch; opens the window when something required is
   *  missing and setup was not skipped. On a PC with one Half-Life install
   *  and no hl.exe set yet, its hl.exe is filled in first (#931), and the
   *  window opens on that step so the user sees what was picked. */
  async function start() {
    const prefilled = await prefillSingleInstall().catch(() => false);
    const checklist = await refresh();
    if (shouldOpenSetupAtLaunch(checklist, setupDismissed)) openWindow(prefilled ? stepIndex('hl') : checklist.nextIndex);
  }

  return { refresh, start, isDismissed: () => setupDismissed };
}
