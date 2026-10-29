// whats_new.js — the What's new window (#1013): each version's release notes
// inside the app, without opening GitHub.
//
// - This version's notes come with the build: the release workflows write
//   the body they publish to `studio/public/release_notes.md`, which Vite
//   copies beside index.html, so they read with no network. A local build has
//   none and says so.
// - Other versions come from GitHub's release list, read through the backend
//   (`release_history`) and saved in the app's data folder. It is read when
//   the window opens only while update checks are on (Studio talks to GitHub
//   then anyway); with them off the window shows the saved copy and a button
//   to read GitHub's list.
// - The update window shows the pending update's notes (`showPendingNotes`).
// - The first start after an update says so in a toast with a button here.
//
// release_notes_view.js reads the Markdown; this module draws it.

import { STRINGS } from './strings.js';
import { getAppVersion, releaseHistory, logFrontendEvent } from './ipc_bridge.js';
import { showToast } from './toast.js';
import {
  parseReleaseNotes, inlineSpans, releaseVersion, releasesForChannel, updatedSince, bundledNotesOrNull,
} from './release_notes_view.js';

const SEEN_KEY = 'dodstudio.whatsNew.seenVersion';
const BUNDLED_URL = '/release_notes.md';

/** Draws `markdown` (a release body) into `container`, replacing what was
 *  there. Returns false, leaving `emptyText`, when the body has no notes. */
export function renderNotesInto(container, markdown, emptyText = STRINGS.WHATS_NEW.NO_NOTES) {
  container.replaceChildren();
  const { blocks, more } = parseReleaseNotes(markdown);
  if (blocks.length === 0 && !more) {
    const p = document.createElement('p');
    p.className = 'release-notes-empty';
    p.textContent = emptyText;
    container.appendChild(p);
    return false;
  }
  for (const block of blocks) {
    if (block.type === 'heading') {
      // The window has its own title; the body's ## and ### sit under it.
      const h = document.createElement(block.level <= 2 ? 'h4' : 'h5');
      appendSpans(h, block.text);
      container.appendChild(h);
    } else if (block.type === 'list') {
      container.appendChild(listOf(block.items));
    } else {
      const p = document.createElement('p');
      appendSpans(p, block.text);
      container.appendChild(p);
    }
  }
  if (more) {
    const details = document.createElement('details');
    details.className = 'release-notes-more';
    const summary = document.createElement('summary');
    summary.textContent = STRINGS.WHATS_NEW.everythingElse(more.summary);
    details.append(summary, listOf(more.items));
    container.appendChild(details);
  }
  return true;
}

function listOf(items) {
  const ul = document.createElement('ul');
  for (const item of items) {
    const li = document.createElement('li');
    appendSpans(li, item);
    ul.appendChild(li);
  }
  return ul;
}

function appendSpans(el, text) {
  for (const span of inlineSpans(text)) {
    if (span.type === 'text') { el.appendChild(document.createTextNode(span.text)); continue; }
    const tag = { bold: 'strong', italic: 'em', code: 'code' }[span.type];
    const node = document.createElement(tag);
    node.textContent = span.text;
    el.appendChild(node);
  }
}

/** The pending update's notes in the update window, before Download &
 *  Install, so "what do I get" has an answer. `notes` null hides them. */
export function showPendingNotes(notes) {
  const box = document.querySelector('#update-modal-notes');
  const body = document.querySelector('#update-modal-notes-body');
  if (!box || !body) return;
  box.hidden = !notes || !renderNotesInto(body, notes);
}

function formatDate(iso) {
  const date = iso ? new Date(iso) : null;
  if (!date || Number.isNaN(date.getTime())) return '';
  return date.toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
}

function readSeen() {
  try { return localStorage.getItem(SEEN_KEY); } catch { return null; }
}

function writeSeen(version) {
  try { localStorage.setItem(SEEN_KEY, version); } catch { /* only a reminder */ }
}

/**
 * The window's state and drawing. `deps` lets tests swap the sources:
 * `getVersion()`, `getChannel()`, `canReadGitHub()` (update checks on),
 * `loadBundled()` (this build's notes or null), `loadHistory(offline)`.
 */
export function createWhatsNew(deps) {
  const modal = document.querySelector('#whats-new-modal');
  const S = STRINGS.WHATS_NEW;
  let history = null; // { releases, from_cache, error } once read this session
  let historyRead = false; // GitHub itself was asked this session
  let readFailed = false; // that ask came back with an error or nothing
  let bundled; // undefined until read; null when the build has none

  function setStatus(text) {
    const el = document.querySelector('#whats-new-status');
    if (el) { el.textContent = text || ''; el.hidden = !text; }
  }

  async function draw() {
    const version = await deps.getVersion();
    const channel = deps.getChannel();
    const current = document.querySelector('#whats-new-current');
    const currentTitle = document.querySelector('#whats-new-current-title');
    const list = document.querySelector('#whats-new-others');
    if (!current || !list) return;
    if (currentTitle) currentTitle.textContent = version ? S.thisVersion(version) : S.THIS_VERSION_UNKNOWN;

    const releases = releasesForChannel(history?.releases || [], channel);
    const own = releases.find((r) => releaseVersion(r) === version);
    if (bundled) renderNotesInto(current, bundled);
    else if (own) renderNotesInto(current, own.body);
    else renderNotesInto(current, '', historyRead ? S.NO_NOTES_FOR_THIS_BUILD : S.NO_BUNDLED_NOTES);

    list.replaceChildren();
    for (const release of releases) {
      if (release === own) continue;
      const details = document.createElement('details');
      details.className = 'whats-new-version';
      const summary = document.createElement('summary');
      const name = document.createElement('span');
      name.className = 'whats-new-version-name';
      name.textContent = S.versionLabel(releaseVersion(release), release.prerelease);
      const date = document.createElement('span');
      date.className = 'whats-new-version-date';
      date.textContent = formatDate(release.published_at);
      summary.append(name, date);
      const body = document.createElement('div');
      body.className = 'release-notes';
      renderNotesInto(body, release.body);
      details.append(summary, body);
      list.appendChild(details);
    }
    const othersTitle = document.querySelector('#whats-new-others-title');
    if (othersTitle) othersTitle.hidden = list.childElementCount === 0;
  }

  async function readHistory(offline) {
    setStatus(offline ? '' : S.STATUS_READING);
    const fetchBtn = document.querySelector('#whats-new-fetch-btn');
    if (fetchBtn) fetchBtn.disabled = true;
    const result = await deps.loadHistory(offline);
    if (fetchBtn) fetchBtn.disabled = false;
    if (result) history = result;
    if (!offline) { historyRead = true; readFailed = !result || Boolean(result.error); }
    if (!result) setStatus(S.STATUS_FAILED);
    else if (result.error) {
      logFrontendEvent(`What's new: GitHub's release list could not be read: ${result.error}`);
      setStatus(result.releases.length ? S.STATUS_SAVED_COPY : S.STATUS_FAILED);
    } else if (offline && !historyRead) {
      setStatus(result.releases.length ? S.STATUS_OFFLINE_SAVED : S.STATUS_OFFLINE_NONE);
    } else setStatus('');
    if (fetchBtn) fetchBtn.hidden = historyRead && !readFailed;
  }

  async function open() {
    if (!modal) return;
    modal.style.display = 'flex';
    if (bundled === undefined) bundled = await deps.loadBundled();
    const online = deps.canReadGitHub();
    const fetchBtn = document.querySelector('#whats-new-fetch-btn');
    // After a failed read the button stays, so a reopened window can retry.
    if (fetchBtn) fetchBtn.hidden = historyRead ? !readFailed : online;
    await draw();
    if (!historyRead) {
      await readHistory(!online);
      await draw();
    }
  }

  async function fetchFromGitHub() {
    await readHistory(false);
    await draw();
  }

  /** On start: the first start after an update offers this window. */
  async function announceIfUpdated() {
    const version = await deps.getVersion();
    if (!version) return;
    const last = readSeen();
    writeSeen(version);
    if (!updatedSince(last, version)) return;
    showToast(S.updatedTo(version), 'info', 15000, { action: { label: S.TOAST_ACTION, onClick: () => open() } });
  }

  return { open, fetchFromGitHub, announceIfUpdated };
}

async function loadBundledNotes() {
  try {
    const response = await fetch(BUNDLED_URL, { cache: 'no-store' });
    if (!response.ok) return null;
    return bundledNotesOrNull(await response.text());
  } catch {
    return null;
  }
}

/** Wires the Help menu's and About's What's new buttons, the window, and the
 *  first-start notice. Called once from main.js after settings load. */
export function startWhatsNew() {
  const whatsNew = createWhatsNew({
    getVersion: getAppVersion,
    getChannel: () => document.querySelector('#config-update-channel')?.value || 'stable',
    canReadGitHub: () => document.querySelector('#config-auto-check-updates')?.checked !== false,
    loadBundled: loadBundledNotes,
    loadHistory: releaseHistory,
  });
  const modal = document.querySelector('#whats-new-modal');
  for (const id of ['#whats-new-menu-btn', '#about-whats-new-btn']) {
    document.querySelector(id)?.addEventListener('click', () => {
      document.querySelector('#about-modal')?.style.setProperty('display', 'none');
      whatsNew.open();
    });
  }
  document.querySelector('#whats-new-fetch-btn')?.addEventListener('click', () => whatsNew.fetchFromGitHub());
  document.querySelector('#whats-new-close-btn')?.addEventListener('click', () => {
    if (modal) modal.style.display = 'none';
  });
  whatsNew.announceIfUpdated();
  return whatsNew;
}
