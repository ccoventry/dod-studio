// pinned_review.js
// The one-time look at the pinned folders (#533). Older builds pinned every
// folder + Add Folder scanned; it now puts them in Recent instead. A pin made
// with the Analyzer's pin toggle is saved in the same list, so the two can't
// be told apart: every pinned folder is listed once, ticked to keep, and the
// user unticks the ones they don't want. `pinned_folders_reviewed` in the
// settings file says the look ran.

import { STRINGS } from './strings.js';

let reviewed = false;

export function getPinnedReviewed() {
  return reviewed;
}

export function setPinnedReviewed(value) {
  reviewed = value === true;
}

/**
 * What to do at launch: 'offer' the list, 'mark' it done with nothing to
 * look at (no pins, so every later pin is a deliberate one), or 'skip' this
 * launch (already done, or the Setup window is open and the list waits for
 * the next launch rather than stacking on top of it).
 */
export function pinnedReviewAction({ reviewed: done, pinned, setupOpen }) {
  if (done) return 'skip';
  if (!pinned || pinned.length === 0) return 'mark';
  return setupOpen ? 'skip' : 'offer';
}

/** The pinned folders the user unticked, in the list's order. */
export function foldersToUnpin(pinned, kept) {
  const keep = new Set(kept);
  return pinned.filter((f) => !keep.has(f));
}

// The list as a modal; resolves to the folders left ticked, or null when
// the user picked Keep all.
function showList(pinned) {
  const S = STRINGS.PINNED_REVIEW;
  const modal = document.createElement('div');
  modal.id = 'pinned-review-modal';
  modal.className = 'modal';
  const content = document.createElement('div');
  content.className = 'modal-content';
  content.style.cssText = 'max-width: 560px; padding: 20px;';
  const title = document.createElement('h2');
  title.textContent = S.TITLE;
  const message = document.createElement('p');
  message.textContent = S.MESSAGE;
  const list = document.createElement('div');
  list.style.cssText = 'display: flex; flex-direction: column; gap: 6px; margin: 10px 0; max-height: 50vh; overflow-y: auto;';
  const boxes = pinned.map((folder) => {
    const label = document.createElement('label');
    label.style.cssText = 'display: flex; gap: 8px; align-items: center; word-break: break-all;';
    label.title = S.KEEP_TITLE;
    const box = document.createElement('input');
    box.type = 'checkbox';
    box.checked = true;
    box.dataset.folder = folder;
    const text = document.createElement('span');
    text.textContent = folder;
    label.append(box, text);
    list.append(label);
    return box;
  });
  const buttons = document.createElement('div');
  buttons.style.cssText = 'display: flex; gap: 8px; margin-top: 16px; justify-content: flex-end;';
  const save = document.createElement('button');
  save.id = 'pinned-review-save-btn';
  save.textContent = S.SAVE_BUTTON;
  const keepAll = document.createElement('button');
  keepAll.id = 'pinned-review-keep-all-btn';
  keepAll.textContent = S.KEEP_ALL_BUTTON;
  buttons.append(save, keepAll);
  content.append(title, message, list, buttons);
  modal.append(content);
  document.body.append(modal);

  return new Promise((resolve) => {
    const close = (result) => {
      modal.remove();
      resolve(result);
    };
    save.addEventListener('click', () => close(boxes.filter((b) => b.checked).map((b) => b.dataset.folder)));
    keepAll.addEventListener('click', () => close(null));
  });
}

/**
 * Runs once per launch, after the Setup window had its chance to open.
 * @param {object} app  getPinned() the pinned list, unpin(folders) takes
 *   folders off it (and saves), isSetupOpen(), persist() saves the settings.
 */
export async function offerPinnedReview({ getPinned, unpin, isSetupOpen = () => false, persist }) {
  const pinned = [...(getPinned() || [])];
  const action = pinnedReviewAction({ reviewed, pinned, setupOpen: isSetupOpen() });
  if (action === 'skip') return;
  if (action === 'mark') {
    reviewed = true;
    await persist();
    return;
  }
  const kept = await showList(pinned);
  reviewed = true;
  const drop = kept ? foldersToUnpin(pinned, kept) : [];
  if (drop.length > 0) await unpin(drop);
  else await persist();
}
