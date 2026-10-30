// capture_history.js
//
// A Highlight Details row's capture history (#439): a ▸ in the row that opens
// a list of every capture of the highlight, newest first, with its settings,
// whether its take is still on disk, the highlights it was recorded with, and
// the renders of that take. The lines come from capture_history_logic.js; this
// draws them and asks the disk (takeCaptureStates) about the takes.

import { STRINGS } from './strings.js';
import { streakUid } from './take_index.js';
import { capturesOf, folderList, describeCapture } from './capture_history_logic.js';
import { highlightStartSeconds } from './highlight_time.js';
import { clockFloor } from './time_format.js';
import { renderSizeText } from './byte_size.js';

const GONE_OPACITY = '0.5';

/** A render's size the way the Render tab writes it. */
function formatSize(bytes) {
  return bytes == null ? STRINGS.RENDER.FILE_SIZE_UNKNOWN : renderSizeText(bytes, 2);
}

function el(tag, text, { title, className } = {}) {
  const node = document.createElement(tag);
  if (text != null) node.textContent = text;
  if (title) node.title = title;
  if (className) node.className = className;
  return node;
}

function revealButton(label, title, path, reveal) {
  const button = el('button', label, { title });
  button.type = 'button';
  button.style.cssText = 'margin-left:6px;font-size:0.9em;cursor:pointer;';
  button.addEventListener('click', () => reveal(path));
  return button;
}

function takeCell(line, reveal) {
  const cell = el('span', STRINGS.CAPTURE_HISTORY.take(line.takeName));
  const note = (text, title) => cell.append(' · ', el('span', text, { title }));
  if (line.status === 'gone') note(STRINGS.CAPTURE_HISTORY.TAKE_GONE, STRINGS.CAPTURE_HISTORY.TAKE_GONE_TITLE);
  else if (line.status === 'unknown') note(STRINGS.CAPTURE_HISTORY.TAKE_FOLDER_UNKNOWN, STRINGS.CAPTURE_HISTORY.TAKE_FOLDER_UNKNOWN_TITLE);
  else if (line.status === 'checking') note(STRINGS.CAPTURE_HISTORY.CHECKING);
  else if (line.status === 'unchecked') note(STRINGS.CAPTURE_HISTORY.CHECK_FAILED);
  else cell.append(revealButton(STRINGS.CAPTURE_HISTORY.OPEN_FOLDER, STRINGS.CAPTURE_HISTORY.OPEN_FOLDER_TITLE, line.folder, reveal));
  return cell;
}

function lineElement(line, reveal) {
  const row = el('div', null, { className: 'capture-history-line' });
  row.dataset.takeKey = line.takeKey;
  row.dataset.takeStatus = line.status;
  row.style.cssText = 'padding:3px 0;';
  // Dim, not hide: a deleted take is still a capture that happened.
  if (line.status === 'gone') row.style.opacity = GONE_OPACITY;
  row.append(el('span', line.when), ' · ', el('span', line.settings), ' · ', takeCell(line, reveal));
  if (line.shared.length) {
    row.append(' · ', el('span', STRINGS.CAPTURE_HISTORY.sharedWith(line.shared), { title: STRINGS.CAPTURE_HISTORY.SHARED_TITLE }));
  }
  const renders = el('div', null, { className: 'capture-history-renders' });
  renders.style.cssText = 'margin-left:18px;font-size:0.9em;opacity:0.85;';
  if (line.status !== 'checking' && line.renders.length === 0) renders.textContent = STRINGS.CAPTURE_HISTORY.NO_RENDERS;
  line.renders.forEach((render) => {
    const item = el('div', render.text, { className: 'capture-history-render' });
    if (render.videoPath) item.append(revealButton(STRINGS.CAPTURE_HISTORY.OPEN_VIDEO, STRINGS.CAPTURE_HISTORY.OPEN_VIDEO_TITLE, render.videoPath, reveal));
    renders.append(item);
  });
  row.append(renders);
  return row;
}

/**
 * `getTakeIndex()` is the project's take index and `getDemos()` the queue (to
 * name the highlights a take is shared with). `fetchStates(folders)` is
 * `takeCaptureStates` and `reveal(path)` is `revealInExplorer`.
 */
export function createCaptureHistory({ getTakeIndex, getDemos, fetchStates, reveal }) {
  // Highlights whose list is open, kept across the pane's re-renders, and the
  // last answers from the disk so a re-render shows them while it asks again.
  const open = new Set();
  const lastStates = new Map();

  function labelOfUid() {
    const names = new Map();
    (getDemos() || []).forEach((demo) => (demo.streaks || []).forEach((streak) => {
      const time = clockFloor(highlightStartSeconds(streak, demo.tickrate));
      names.set(streakUid(demo.path, streak), STRINGS.CAPTURE_HISTORY.otherHighlight(time, demo.is_pov ? '' : streak.target_player));
    }));
    return (uid) => names.get(uid) || null;
  }

  function draw(listCell, uid, captures, states) {
    const label = labelOfUid();
    const context = { labelOfUid: label, formatSize };
    listCell.replaceChildren(...captures.map((capture, i) =>
      lineElement(describeCapture(capture, uid, states?.[i], context), reveal)));
  }

  async function fill(listCell, uid, captures) {
    draw(listCell, uid, captures, lastStates.get(uid));
    const answer = await fetchStates(folderList(captures));
    // null = the check failed; each line says so rather than staying "checking".
    const states = answer ? answer.map((state, i) => (captures[i].record.take_folder ? state : undefined)) : captures.map(() => null);
    lastStates.set(uid, states);
    if (listCell.isConnected) draw(listCell, uid, captures, states);
  }

  function showList(tr, uid, captures) {
    const row = document.createElement('tr');
    row.className = 'capture-history-row';
    const cell = document.createElement('td');
    cell.colSpan = tr.children.length;
    cell.style.cssText = 'padding:4px 8px 8px 32px;font-size:0.9em;background:#1b1b1b;';
    row.append(cell);
    tr.after(row);
    fill(cell, uid, captures);
    return row;
  }

  /** Adds the ▸ to a highlight's row, when it was captured at least once. */
  function attach(tr, demo, streak) {
    const uid = streakUid(demo.path, streak);
    const captures = capturesOf(getTakeIndex?.(), uid);
    if (captures.length === 0) return;

    const toggle = el('button', null, { title: STRINGS.CAPTURE_HISTORY.TOGGLE_TITLE, className: 'capture-history-toggle' });
    toggle.type = 'button';
    toggle.style.cssText = 'background:transparent;border:none;color:inherit;cursor:pointer;padding:0 4px 0 0;';
    let listRow = null;
    const sync = () => { toggle.textContent = open.has(uid) ? '▾' : '▸'; };
    const set = (wantOpen) => {
      if (wantOpen) {
        open.add(uid);
        listRow = showList(tr, uid, captures);
      } else {
        open.delete(uid);
        listRow?.remove();
        listRow = null;
      }
      sync();
    };
    toggle.addEventListener('click', () => set(!open.has(uid)));
    tr.firstElementChild.prepend(toggle);
    sync();
    // The pane rebuilds its rows after every edit; an open list stays open.
    if (open.has(uid)) listRow = showList(tr, uid, captures);
  }

  return { attach };
}
