// movie_copy_ui.js
// Setup's Make a movie copy dialog (#939): a name, "Also copy my demos",
// the size against the drive's free space, then Copy with a bar by bytes
// and Cancel (asked first; the half-made folder goes). The rules are
// movie_copy.js's; the copy is native::shared::movie_copy's.
import { listen } from '@tauri-apps/api/event';
import { movieCopyPlan, startMovieCopy, cancelMovieCopy } from './ipc_bridge.js';
import { themedConfirm } from './themed_confirm.js';
import { escapeHtml } from './html.js';
import { STRINGS } from './strings.js';
import { copyCheck, suggestCopyName, progressPercent, progressLine, DEFAULT_COPY_NAME } from './movie_copy.js';

/**
 * @param {object} deps
 *   getHlPath(): the Half-Life Executable as set.
 *   onDone(folder): after a copy finishes; sets the Game Folder to it.
 *   confirm, ipc, listenTo: stand-ins for tests (themedConfirm, the IPC
 *   bridge, Tauri's listen).
 * @returns {{ open, isOpen, isRunning }}
 */
export function createMovieCopy({
  getHlPath,
  onDone,
  confirm = themedConfirm,
  ipc = { movieCopyPlan, startMovieCopy, cancelMovieCopy },
  listenTo = listen,
}) {
  const S = STRINGS.SETUP.MOVIE_COPY;
  let modal = null;
  // 'measuring' | 'ready' | 'copying' | 'cancelling' | 'finished'
  let state = 'measuring';
  let plan = null;
  let gamePath = '';
  // The newest open() wins a slow measurement.
  let generation = 0;

  const el = (id) => modal.querySelector(`#${id}`);

  function build() {
    modal = document.createElement('div');
    modal.id = 'movie-copy-modal';
    modal.className = 'modal';
    modal.style.display = 'none';
    modal.innerHTML = `<div class="modal-content movie-copy-content" role="dialog" aria-labelledby="movie-copy-title">
        <h2 id="movie-copy-title">${escapeHtml(S.TITLE)}</h2>
        <p class="setup-muted">${escapeHtml(S.INTRO)}</p>
        <label class="setup-field"><span class="setup-field-label">${escapeHtml(S.NAME_LABEL)}</span>
          <input type="text" id="movie-copy-name" spellcheck="false" autocomplete="off" maxlength="64" /></label>
        <div id="movie-copy-name-problem" class="setup-note warn" hidden></div>
        <label class="movie-copy-check"><input type="checkbox" id="movie-copy-demos" /> ${escapeHtml(S.DEMOS_LABEL)}</label>
        <div id="movie-copy-size" class="setup-muted"></div>
        <div id="movie-copy-space" class="setup-muted"></div>
        <div id="movie-copy-progress" class="progress-bar-container" hidden><div class="progress-bar-fill" style="width: 0%;"></div></div>
        <div id="movie-copy-status" class="movie-copy-status"></div>
        <div class="setup-footer">
          <span class="setup-footer-spacer"></span>
          <button type="button" id="movie-copy-cancel" hidden>${escapeHtml(S.CANCEL)}</button>
          <button type="button" id="movie-copy-close">${escapeHtml(S.CLOSE)}</button>
          <button type="button" id="movie-copy-go" class="primary-btn">${escapeHtml(S.COPY)}</button>
        </div>
      </div>`;
    document.body.append(modal);
    el('movie-copy-name').addEventListener('input', draw);
    el('movie-copy-demos').addEventListener('change', draw);
    el('movie-copy-go').addEventListener('click', copy);
    el('movie-copy-cancel').addEventListener('click', askCancel);
    el('movie-copy-close').addEventListener('click', close);
  }

  function setStatus(text, kind = '') {
    const status = el('movie-copy-status');
    status.textContent = text;
    status.className = `movie-copy-status${kind ? ` setup-note ${kind}` : ''}`;
  }

  // Redraws the checks and which controls work from `state`.
  function draw() {
    const name = el('movie-copy-name').value;
    const demos = el('movie-copy-demos').checked;
    const check = copyCheck(plan, name, demos);
    const running = state === 'copying' || state === 'cancelling';
    const problem = el('movie-copy-name-problem');
    const showProblem = state === 'ready' && !!plan && !!check.nameProblem;
    problem.hidden = !showProblem;
    problem.textContent = showProblem ? check.nameProblem : '';
    el('movie-copy-size').textContent = plan ? [check.sizeLine, demos ? '' : check.demosLine].filter(Boolean).join(' ') : '';
    const space = el('movie-copy-space');
    space.textContent = plan ? check.spaceLine : '';
    space.className = plan && !check.fits ? 'setup-note warn' : 'setup-muted';
    el('movie-copy-name').disabled = state !== 'ready';
    el('movie-copy-demos').disabled = state !== 'ready';
    const go = el('movie-copy-go');
    go.hidden = state === 'finished';
    go.disabled = state !== 'ready' || !check.canCopy;
    const cancel = el('movie-copy-cancel');
    cancel.hidden = !running;
    cancel.disabled = state === 'cancelling';
    el('movie-copy-close').disabled = running;
  }

  function showProgress(p) {
    const bar = el('movie-copy-progress');
    bar.hidden = false;
    bar.querySelector('.progress-bar-fill').style.width = `${progressPercent(p)}%`;
    if (state === 'copying') setStatus(progressLine(p));
  }

  async function measure() {
    const mine = ++generation;
    state = 'measuring';
    plan = null;
    setStatus(S.LOOKING);
    draw();
    let next = null;
    try {
      next = await ipc.movieCopyPlan(gamePath);
    } catch (err) {
      if (mine !== generation) return;
      state = 'ready';
      setStatus(S.lookFailed(String(err)), 'warn');
      draw();
      return;
    }
    if (mine !== generation) return;
    plan = next;
    state = 'ready';
    const input = el('movie-copy-name');
    if (!input.value || !input.dataset.typed) {
      input.value = suggestCopyName(plan?.existing, plan?.default_name || DEFAULT_COPY_NAME);
    }
    setStatus('');
    draw();
  }

  async function copy() {
    if (state !== 'ready') return;
    const name = el('movie-copy-name').value;
    const demos = el('movie-copy-demos').checked;
    if (!copyCheck(plan, name, demos).canCopy) return;
    state = 'copying';
    setStatus(S.STARTING);
    showProgress({ done_bytes: 0, total_bytes: 1, done_files: 0, total_files: 1 });
    draw();
    let unlisten = () => {};
    try {
      unlisten = await listenTo('movie_copy_progress', (event) => showProgress(event.payload));
    } catch (err) {
      console.error('Error listening for movie copy progress:', err);
    }
    let outcome = null;
    let failure = null;
    try {
      outcome = await ipc.startMovieCopy(gamePath, name, demos);
    } catch (err) {
      failure = String(err);
    }
    unlisten();
    el('movie-copy-progress').hidden = true;
    if (outcome?.state === 'done') {
      state = 'finished';
      setStatus(S.done(outcome.folder), 'ok');
      draw();
      try {
        await onDone(outcome.folder);
      } catch (err) {
        console.error('Error picking the movie copy as the game folder:', err);
      }
      return;
    }
    // Cancelled or failed: the name is free again, the sizes the same.
    state = 'ready';
    if (outcome?.state === 'cancelled') setStatus(S.CANCELLED, 'info');
    else setStatus(S.failed(failure || ''), 'warn');
    draw();
  }

  async function askCancel() {
    if (state !== 'copying') return;
    const yes = await confirm(S.CANCEL_ASK, {
      title: S.CANCEL_ASK_TITLE, confirmLabel: S.CANCEL_YES, cancelLabel: S.CANCEL_NO,
    });
    // The copy may have finished while the question was up.
    if (!yes || state !== 'copying') return;
    state = 'cancelling';
    setStatus(S.CANCELLING);
    draw();
    ipc.cancelMovieCopy().catch(() => {});
  }

  function isOpen() {
    return !!modal && modal.style.display !== 'none';
  }

  function isRunning() {
    return state === 'copying' || state === 'cancelling';
  }

  function close() {
    if (!modal || isRunning()) return;
    modal.style.display = 'none';
  }

  /** Opens the dialog for the `dod` beside hl.exe and measures it. */
  function open() {
    if (!modal) build();
    if (isRunning()) {
      modal.style.display = 'flex';
      return;
    }
    gamePath = getHlPath();
    const input = el('movie-copy-name');
    input.value = '';
    delete input.dataset.typed;
    el('movie-copy-demos').checked = false;
    el('movie-copy-progress').hidden = true;
    modal.style.display = 'flex';
    measure();
    input.addEventListener('input', () => { input.dataset.typed = '1'; }, { once: true });
  }

  return { open, isOpen, isRunning };
}
