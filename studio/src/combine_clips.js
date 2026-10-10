// combine_clips.js
// Render Studio's Combine Clips window (#107): put rendered clips in order and
// join them into one video. Clips that all match are joined without
// re-encoding; mixed ones are fitted to the first clip and re-encoded
// (native::hlcr::combine). The Rough Cut page idea (#585) builds on this.

import { listen } from '@tauri-apps/api/event';
import { open, save } from '@tauri-apps/plugin-dialog';
import { combinePlan, combineClips, combineCancel, revealInExplorer } from './ipc_bridge.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

const VIDEO_EXTENSIONS = ['mp4', 'mov', 'mkv', 'avi'];

const $ = (selector) => document.querySelector(selector);
const fileName = (path) => String(path).split(/[\\/]/).pop();

/** Adds `paths` to `list`, leaving out any already in it (case-insensitive). */
export function addClips(list, paths) {
  const seen = new Set(list.map((p) => p.toLowerCase()));
  const out = [...list];
  for (const p of paths || []) {
    if (p && !seen.has(p.toLowerCase())) {
      seen.add(p.toLowerCase());
      out.push(p);
    }
  }
  return out;
}

/** `list` with the clip at `index` moved by `delta` (kept in range). */
export function moveClip(list, index, delta) {
  const to = Math.max(0, Math.min(list.length - 1, index + delta));
  if (to === index) return list;
  const out = [...list];
  const [clip] = out.splice(index, 1);
  out.splice(to, 0, clip);
  return out;
}

/** Where the joined video goes by default: beside the first clip, named
 *  after it, in its container when nothing is re-encoded, else MP4. */
export function defaultOutput(first, streamCopy) {
  const name = fileName(first);
  const dot = name.lastIndexOf('.');
  const stem = dot > 0 ? name.slice(0, dot) : name;
  const ext = streamCopy && dot > 0 ? name.slice(dot + 1) : 'mp4';
  const folder = String(first).slice(0, String(first).length - name.length);
  return `${folder}${stem}_combined.${ext}`;
}

/** Seconds as m:ss. */
function clock(secs) {
  const s = Math.round(secs || 0);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

/**
 * Wires the window. `finishedRenders()` gives the output files of this
 * session's finished render jobs; `ffmpegPath()` the FFmpeg override, if any.
 */
export function initCombineClips({ finishedRenders, ffmpegPath }) {
  const modal = $('#combine-modal');
  const listEl = $('#combine-list');
  const planEl = $('#combine-plan');
  const startBtn = $('#combine-start-btn');
  const cancelBtn = $('#combine-cancel-btn');
  const progress = $('#combine-progress');
  const fill = $('#combine-progress-fill');
  if (!modal || !listEl) return;

  let clips = [];
  let plan = null;
  let planToken = 0;
  let running = false;

  function setRunning(value) {
    running = value;
    cancelBtn.disabled = !running;
    progress.hidden = !running;
    if (!running) fill.style.width = '0%';
    modal.querySelectorAll('[data-combine-edit]').forEach((b) => { b.disabled = running; });
    startBtn.disabled = running || !plan || clips.length < 2;
  }

  async function refreshPlan() {
    const token = ++planToken;
    plan = null;
    startBtn.disabled = true;
    if (clips.length < 2) {
      planEl.textContent = STRINGS.COMBINE.NEED_TWO;
      return;
    }
    planEl.textContent = STRINGS.COMBINE.CHECKING;
    try {
      const result = await combinePlan(clips, ffmpegPath());
      if (token !== planToken) return;
      plan = result;
      planEl.textContent = result.stream_copy
        ? STRINGS.COMBINE.planCopy(clock(result.total_secs))
        : STRINGS.COMBINE.planEncode(clock(result.total_secs), result.clips[0].width, result.clips[0].height, result.clips[0].fps);
    } catch (err) {
      if (token !== planToken) return;
      planEl.textContent = STRINGS.COMBINE.planFailed(err);
    }
    setRunning(running);
  }

  function render() {
    listEl.replaceChildren(...clips.map((path, i) => {
      const li = document.createElement('li');
      li.className = 'combine-row';
      const name = document.createElement('span');
      name.className = 'combine-name';
      name.textContent = fileName(path);
      name.title = path;
      const button = (text, title, onClick, disabled = false) => {
        const b = document.createElement('button');
        b.type = 'button';
        b.textContent = text;
        b.title = title;
        b.dataset.combineEdit = '';
        b.disabled = disabled || running;
        b.addEventListener('click', onClick);
        return b;
      };
      li.append(
        name,
        button('↑', STRINGS.COMBINE.MOVE_UP, () => setClips(moveClip(clips, i, -1)), i === 0),
        button('↓', STRINGS.COMBINE.MOVE_DOWN, () => setClips(moveClip(clips, i, 1)), i === clips.length - 1),
        button('✕', STRINGS.COMBINE.REMOVE, () => setClips(clips.filter((_, j) => j !== i))),
      );
      return li;
    }));
    if (!clips.length) {
      const li = document.createElement('li');
      li.className = 'combine-empty';
      li.textContent = STRINGS.COMBINE.EMPTY;
      listEl.appendChild(li);
    }
  }

  function setClips(next) {
    clips = next;
    render();
    refreshPlan();
  }

  $('#combine-open-btn')?.addEventListener('click', () => {
    modal.style.display = 'flex';
    if (!clips.length) setClips(addClips([], finishedRenders()));
    else refreshPlan();
  });
  $('#combine-close-btn')?.addEventListener('click', () => {
    if (!running) modal.style.display = 'none';
  });
  $('#combine-add-finished-btn')?.addEventListener('click', () => {
    const found = finishedRenders();
    if (!found.length) showToast(STRINGS.COMBINE.NO_FINISHED, 'info');
    setClips(addClips(clips, found));
  });
  $('#combine-add-files-btn')?.addEventListener('click', async () => {
    try {
      const picked = await open({
        multiple: true,
        title: STRINGS.COMBINE.ADD_FILES_TITLE,
        filters: [{ name: STRINGS.COMBINE.VIDEO_FILTER, extensions: VIDEO_EXTENSIONS }],
      });
      if (picked) setClips(addClips(clips, Array.isArray(picked) ? picked : [picked]));
    } catch (err) {
      console.error('Choosing clips to combine failed:', err);
    }
  });
  $('#combine-clear-btn')?.addEventListener('click', () => setClips([]));

  startBtn.addEventListener('click', async () => {
    if (!plan || clips.length < 2) return;
    let output;
    try {
      output = await save({
        title: STRINGS.COMBINE.SAVE_TITLE,
        defaultPath: defaultOutput(clips[0], plan.stream_copy),
        filters: [{ name: STRINGS.COMBINE.VIDEO_FILTER, extensions: VIDEO_EXTENSIONS }],
      });
    } catch (err) {
      console.error('Choosing where to save the combined video failed:', err);
      return;
    }
    if (!output) return;
    setRunning(true);
    try {
      await combineClips(clips, output, ffmpegPath());
      showToast(STRINGS.COMBINE.saved(fileName(output)), 'success', 6000, {
        action: { label: STRINGS.COMBINE.SHOW_FILE, onClick: () => revealInExplorer(output).catch(() => {}) },
      });
    } catch {
      // ipc_bridge already said why (and nothing for a cancel).
    } finally {
      setRunning(false);
    }
  });
  cancelBtn.addEventListener('click', () => combineCancel().catch(() => {}));

  listen('combine_progress', (event) => {
    if (running) fill.style.width = `${Math.round((event.payload?.fraction || 0) * 100)}%`;
  });

  render();
  setRunning(false);
}
