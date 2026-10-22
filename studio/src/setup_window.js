// setup_window.js
// The Setup window (#599): six steps, one at a time, each with a status and a
// way past it. It draws from the snapshot setup_wiring.js keeps and calls back
// for every action (pickers, the FFmpeg link, the demo scan), so it does no
// IPC of its own. Step 1 explains how to make a movie copy of the dod folder;
// it never copies anything itself.
import { STRINGS } from './strings.js';
import { escapeHtml } from './html.js';
import {
  SETUP_STEPS,
  defaultDemoFolder,
  driveLabel,
  formatBytes,
  EXE_NAMES,
  usesPlayFolder,
  sameDriveFolders,
  setupChecklist,
  wrongExeName,
} from './setup_checklist.js';

/**
 * @param {object} deps
 *   getSnapshot(): the current state (setup_wiring.js's snapshot).
 *   pickHl(), pickHlae(), linkFfmpeg(), openHlaePage(): step actions.
 *   pickFfmpeg(), useHlaeFfmpeg(), openFfmpegPage(): the FFmpeg step's own.
 *   setGameFolder(name): the Half-Life step's game folder pick.
 *   addFolder(kind), removeFolder(kind, path): kind is 'captures' | 'renders'.
 *   pickDemoFolder(): resolves to a folder or null.
 *   onSkip(): "Skip setup for now". onFinish(demoFolder): the last step's
 *   Finish, with the folder to scan ('' for none).
 */
export function createSetupWindow(deps) {
  const modal = document.querySelector('#setup-modal');
  const stepsEl = document.querySelector('#setup-steps');
  const titleEl = document.querySelector('#setup-step-title');
  const contentEl = document.querySelector('#setup-step-content');
  const backBtn = document.querySelector('#setup-back-btn');
  const nextBtn = document.querySelector('#setup-next-btn');
  const skipBtn = document.querySelector('#setup-skip-btn');
  const S = STRINGS.SETUP;

  let index = 0;
  // The demo folder Finish scans. Starts as the movie copy's dod folder.
  let demoFolder = null;

  function isOpen() {
    return !!modal && modal.style.display !== 'none';
  }

  function open(step = 0) {
    if (!modal) return;
    index = typeof step === 'number' ? step : Math.max(0, SETUP_STEPS.indexOf(step));
    demoFolder = null;
    modal.style.display = 'flex';
    render();
  }

  function close() {
    if (modal) modal.style.display = 'none';
  }

  function status(kind, text) {
    return `<span class="setup-status"><span class="setup-dot ${kind}"></span>${escapeHtml(text)}</span>`;
  }

  function pathRow(path, buttonId, buttonLabel) {
    const shown = path
      ? `<span class="setup-path">${escapeHtml(path)}</span>`
      : `<span class="setup-path empty">${escapeHtml(S.NOT_SET)}</span>`;
    return `<div class="setup-path-row">${shown}<button type="button" id="${buttonId}">${escapeHtml(buttonLabel)}</button></div>`;
  }

  function exeStatus(path, state, foundText, expected) {
    if (!path) return '';
    if (state === 'not_found') return status('bad', S.PATH_NOT_FOUND);
    if (state === 'not_a_file') return status('bad', S.PATH_IS_A_FOLDER);
    const wrong = wrongExeName(path, expected);
    if (wrong) return status('bad', S.wrongExe(wrong, expected));
    return status('ok', foundText);
  }

  function hlStep(snap) {
    const build = snap.hlBuild && !wrongExeName(snap.hlPath, EXE_NAMES.hl) ?`<span class="setup-chip ${snap.hlBuild === 'anniversary' ? 'ok' : 'info'}">${escapeHtml(S.HL_BUILD[snap.hlBuild])}</span>` : '';
    const playWarn = usesPlayFolder(snap.hlPath, snap.gameFolder)
      ? `<div class="setup-note warn" id="setup-hl-play-warning">${escapeHtml(S.HL_DOD_FOLDER)}</div>` : '';
    return `<p>${escapeHtml(S.HL_INTRO)}</p>
      <div class="setup-note info">${escapeHtml(S.HL_RECOMMEND)}</div>
      <div class="setup-field">
        <div class="setup-field-label">${escapeHtml(S.HL_FIELD)} ${exeStatus(snap.hlPath, snap.hlState, S.HL_FOUND, EXE_NAMES.hl)} ${build}</div>
        ${pathRow(snap.hlPath, 'setup-hl-browse', S.BROWSE)}
      </div>
      ${gameFolderField(snap)}${playWarn}`;
  }

  // The same setting as Configuration > Paths > Game Folder: the folders
  // beside hl.exe that hold a game, so a dod_movies copy is one pick away.
  function gameFolderField(snap) {
    const current = snap.gameFolder || 'dod';
    const folders = snap.gameFolders || [];
    const select = folders.length
      ? `<select id="setup-game-folder">${folders.map((f) =>
          `<option value="${escapeHtml(f)}"${f.toLowerCase() === current.toLowerCase() ? ' selected' : ''}>${escapeHtml(f)}</option>`).join('')}</select>`
      : `<span class="setup-path empty">${escapeHtml(S.GAME_FOLDER_NONE)}</span>`;
    return `<div class="setup-field">
        <div class="setup-field-label">${escapeHtml(S.GAME_FOLDER_FIELD)}</div>
        <div class="setup-path-row">${select}</div>
      </div>`;
  }

  function hlaeStep(snap) {
    return `<p>${escapeHtml(S.HLAE_INTRO)}</p>
      <div class="setup-btn-row"><button type="button" id="setup-hlae-get">${escapeHtml(S.HLAE_GET)}</button></div>
      <div class="setup-field">
        <div class="setup-field-label">${escapeHtml(S.HLAE_FIELD)} ${exeStatus(snap.hlaePath, snap.hlaeState, S.HLAE_FOUND, EXE_NAMES.hlae)}</div>
        ${pathRow(snap.hlaePath, 'setup-hlae-browse', S.BROWSE)}
      </div>`;
  }

  function ffmpegStep(snap) {
    const intro = `<p>${escapeHtml(S.FFMPEG_INTRO)}</p>`;
    if (!snap.hlaePath) return `${intro}<div class="setup-note info">${escapeHtml(S.FFMPEG_NEEDS_HLAE)}</div>`;
    if (snap.ffmpegUsable == null) return `${intro}${status('', S.FFMPEG_CHECKING)}`;

    // DoD Studio's own FFmpeg first: Render and Finish clips use it, and the
    // link below writes it into HLAE's ini. A fresh install has only the bare
    // name `ffmpeg`, which most unzipped copies are not on PATH for.
    let appStatus;
    if (!snap.appFfmpeg) appStatus = status('warn', S.FFMPEG_APP_MISSING);
    else if (snap.appFfmpegProblem) appStatus = status('bad', S.ffmpegAppProblem(snap.appFfmpegProblem));
    else appStatus = status('ok', S.FFMPEG_APP_FOUND);
    let body = `<div class="setup-btn-row"><button type="button" id="setup-ffmpeg-get">${escapeHtml(S.FFMPEG_GET)}</button></div>
      <div class="setup-field">
        <div class="setup-field-label">${escapeHtml(S.FFMPEG_FIELD)} ${appStatus}</div>
        ${pathRow(snap.appFfmpeg || '', 'setup-ffmpeg-browse', S.BROWSE)}
      </div>`;
    if (!snap.appFfmpeg && snap.hlaeBundledFfmpeg) {
      body += `<div class="setup-note info">${escapeHtml(S.ffmpegHlaeHasOne(snap.hlaeBundledFfmpeg))}</div>
        <div class="setup-btn-row"><button type="button" id="setup-ffmpeg-use-hlae">${escapeHtml(S.FFMPEG_USE_HLAE)}</button></div>`;
    }

    let hlae;
    if (snap.ffmpegUsable) hlae = status('ok', S.FFMPEG_FOUND);
    else {
      hlae = status('warn', S.FFMPEG_MISSING);
      if (snap.ffmpegCanLink && snap.appFfmpeg && !snap.appFfmpegProblem) {
        hlae += `<div class="setup-btn-row"><button type="button" id="setup-ffmpeg-link">${escapeHtml(STRINGS.CAPTURE_CONFIG.HLAE_FFMPEG_LINK_BUTTON)}</button></div>
          <p class="setup-muted">${escapeHtml(S.FFMPEG_LINK_NOTE)}</p>`;
      } else if (snap.ffmpegCanLink) {
        hlae += `<p class="setup-muted">${escapeHtml(S.FFMPEG_PICK_FIRST)}</p>`;
      }
    }
    body += `<div class="setup-field"><div class="setup-field-label">${escapeHtml(S.FFMPEG_HLAE_LABEL)}</div>${hlae}</div>`;
    return `${intro}${body}`;
  }

  function folderStep(kind, snap) {
    const list = (kind === 'captures' ? snap.targetDrives : snap.renderExportDirs) || [];
    const rows = list.map((f) => {
      let free = '';
      if (typeof f.freeBytes === 'number') {
        free = f.freeBytes > 0 ? S.freeOn(formatBytes(f.freeBytes), driveLabel(f.path)) : S.NO_SPACE_FOUND;
      }
      return `<div class="setup-path-row"><span class="setup-path">${escapeHtml(f.path)}<span class="setup-free">${escapeHtml(free)}</span></span>
        <button type="button" class="setup-folder-remove" data-path="${escapeHtml(f.path)}">${escapeHtml(S.REMOVE)}</button></div>`;
    }).join('') || `<div class="setup-path-row"><span class="setup-path empty">${escapeHtml(S.NO_FOLDERS_YET)}</span></div>`;
    const intro = kind === 'captures' ? S.CAPTURES_INTRO : S.RENDERS_INTRO;
    const field = kind === 'captures' ? S.CAPTURES_FIELD : S.RENDERS_FIELD;
    const sameDrive = sameDriveFolders(list.map((f) => f.path))
      .map((r) => `<div class="setup-note warn setup-same-drive">${escapeHtml(S.sameDrive(r.path, r.drive, r.first))}</div>`)
      .join('');
    return `<p>${escapeHtml(intro)}</p>
      <div class="setup-field"><div class="setup-field-label">${escapeHtml(field)}</div>${rows}</div>${sameDrive}
      <div class="setup-btn-row"><button type="button" id="setup-folder-add">${escapeHtml(list.length ? S.ADD_ANOTHER_DRIVE : S.ADD_FOLDER)}</button></div>`;
  }

  function demosStep(snap, done) {
    // Suggested only while no demos are set up: someone who already has a
    // queue should not get a whole dod folder scanned into it by Finish.
    if (demoFolder == null) demoFolder = done ? '' : defaultDemoFolder(snap.hlPath);
    const shown = demoFolder
      ? `<span class="setup-path">${escapeHtml(demoFolder)}</span>`
      : `<span class="setup-path empty">${escapeHtml(S.DEMOS_NONE)}</span>`;
    return `<p>${escapeHtml(S.DEMOS_INTRO)}</p>
      <div class="setup-field"><div class="setup-field-label">${escapeHtml(S.DEMOS_FIELD)}</div>
        <div class="setup-path-row">${shown}<button type="button" id="setup-demos-browse">${escapeHtml(S.BROWSE)}</button></div>
      </div>
      ${demoFolder ? `<p class="setup-muted">${escapeHtml(S.DEMOS_FINISH_NOTE)}</p>` : ''}`;
  }

  const STEP_TITLES = {
    hl: S.HL_TITLE, hlae: S.HLAE_TITLE, ffmpeg: S.FFMPEG_TITLE,
    captures: S.CAPTURES_TITLE, renders: S.RENDERS_TITLE, demos: S.DEMOS_TITLE,
  };

  function render() {
    if (!isOpen()) return;
    const snap = deps.getSnapshot();
    const checklist = setupChecklist(snap);
    const key = SETUP_STEPS[index];

    stepsEl.innerHTML = checklist.steps.map((s, i) => {
      const cls = [s.done ? 'done' : '', i === index ? 'now' : ''].filter(Boolean).join(' ');
      const tick = s.done ? '✓ ' : '';
      return `<li class="${cls}" data-index="${i}" title="${escapeHtml(s.required ? S.REQUIRED : S.OPTIONAL)}">${tick}${escapeHtml(S.STEP_TABS[s.key])}</li>`;
    }).join('');
    stepsEl.querySelectorAll('li').forEach((li) => {
      li.addEventListener('click', () => { index = Number(li.dataset.index); render(); });
    });

    titleEl.textContent = `${index + 1}. ${STEP_TITLES[key]}`;
    if (key === 'hl') contentEl.innerHTML = hlStep(snap);
    else if (key === 'hlae') contentEl.innerHTML = hlaeStep(snap);
    else if (key === 'ffmpeg') contentEl.innerHTML = ffmpegStep(snap);
    else if (key === 'captures' || key === 'renders') contentEl.innerHTML = folderStep(key, snap);
    else contentEl.innerHTML = demosStep(snap, checklist.steps[index].done);

    const on = (id, fn) => contentEl.querySelector(`#${id}`)?.addEventListener('click', fn);
    on('setup-hl-browse', () => deps.pickHl());
    contentEl.querySelector('#setup-game-folder')
      ?.addEventListener('change', (e) => deps.setGameFolder(e.target.value));
    on('setup-hlae-browse', () => deps.pickHlae());
    on('setup-hlae-get', () => deps.openHlaePage());
    on('setup-ffmpeg-get', () => deps.openFfmpegPage());
    on('setup-ffmpeg-browse', () => deps.pickFfmpeg());
    on('setup-ffmpeg-use-hlae', () => deps.useHlaeFfmpeg());
    on('setup-ffmpeg-link', async (e) => {
      e.currentTarget.disabled = true;
      await deps.linkFfmpeg();
    });
    on('setup-folder-add', () => deps.addFolder(key));
    contentEl.querySelectorAll('.setup-folder-remove').forEach((btn) => {
      btn.addEventListener('click', () => deps.removeFolder(key, btn.dataset.path));
    });
    on('setup-demos-browse', async () => {
      const picked = await deps.pickDemoFolder();
      if (picked) { demoFolder = picked; render(); }
    });

    const last = index === SETUP_STEPS.length - 1;
    backBtn.disabled = index === 0;
    nextBtn.textContent = last ? S.FINISH : (checklist.steps[index].done ? S.NEXT : S.SKIP_STEP);
  }

  backBtn?.addEventListener('click', () => {
    if (index > 0) { index -= 1; render(); }
  });
  nextBtn?.addEventListener('click', () => {
    if (index < SETUP_STEPS.length - 1) { index += 1; render(); return; }
    close();
    deps.onFinish(demoFolder || '');
  });
  skipBtn?.addEventListener('click', () => {
    close();
    deps.onSkip();
  });

  return { open, close, isOpen, render };
}
