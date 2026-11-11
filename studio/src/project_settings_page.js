// project_settings_page.js
// The Project settings page (#1085, build plan step 6): a full page over the
// app, opened from File › Project Settings, with the tabs Project, Clip
// settings and Players and sprays and one OK, Cancel and Apply for all of
// them. Edits wait here until OK or Apply puts them into the open project
// (project_settings.js), which is then unsaved; Save writes the file. Cancel
// drops what changed since the last Apply. Leaving any other way with
// pending edits asks Apply, Discard or Stay. The page never touches the
// Configuration form, which is the app's store (settings_sync.js).
//
// Only the Project tab is built (project_settings_project_tab.js); the other
// two arrive with build plan steps 8 and 7 and show greyed until then.
//
// Hidden behind the New UI switch (new_ui.js) until the whole rework ships:
// with it off there is no menu item and open() does nothing.

import './project_settings_page.css';
import { logFrontendEvent } from './ipc_bridge.js';
import { blockForPage, applyBlockFromPage, onProjectBlockReplaced, setProjectSettingsPageLeave } from './project_settings.js';
import {
  startingValues, changedKeys, pendingProblems, blockWithPending,
} from './project_settings_page_logic.js';
import { createProjectTab, showProblems, PROJECT_SECTIONS } from './project_settings_project_tab.js';
import { setAppShortcutsEnabled } from './app_shortcuts.js';
import {
  projectSettingsPageOpenedLine, projectSettingsPageAppliedLine, projectSettingsPageClosedLine,
} from './project_log.js';
import { fileNameOf } from './path_display.js';
import { isNewUiOn, onNewUiChange } from './new_ui.js';
import { STRINGS } from './strings.js';

const S = STRINGS.PROJECT_SETTINGS_PAGE;

/** An element with `props` (class, text, title, any other property). */
function el(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (value === undefined || value === null) continue;
    if (key === 'class') node.className = value;
    else if (key === 'text') node.textContent = value;
    else node[key] = value;
  }
  node.append(...children.filter(Boolean));
  return node;
}

/** Keys the app's own shortcuts swallow (app_shortcuts.js): let through to
 *  it, so a reload key never reloads the window from inside the page. */
function isSwallowedAppKey(e) {
  const key = (e.key || '').toLowerCase();
  return e.key === 'F5' || ((e.ctrlKey || e.metaKey) && ['n', 'o', 's', 'w', 'r'].includes(key));
}

/**
 * @param {object} app  `getProjectPath()` the open project's file, or null
 *   for a session not saved yet; `onSettingsInEffectChange()` redraws what
 *   reads the settings in effect (the capture summary, the clip names shown), after
 *   an Apply and after a project opens or closes;
 *   `isBatchRunning()` whether a capture batch runs, for the banner.
 * @returns {{ open: () => void, isOpen: () => boolean, leave: (how: string) => Promise<boolean> }}
 */
export function createProjectSettingsPage({ getProjectPath, onSettingsInEffectChange = () => {}, isBatchRunning = () => false }) {
  const navbar = document.querySelector('#top-navbar');
  let root = null;
  let tabBody = null;
  let footer = null;
  let banner = null;
  let kicker = null;
  // While open: the values the page started from (or last applied), the
  // pending ones, the block and form they came from, and which Custom boxes
  // are open (`ui`).
  let state = null;
  let leavePrompt = null;

  const projectTab = createProjectTab({ getState: () => state, onEdit });

  function onEdit(changes, { redraw = false } = {}) {
    if (!state) return;
    state.pending = { ...state.pending, ...changes };
    if (redraw) projectTab.render(tabBody);
    refresh();
  }

  /** Apply's and OK's state, and the problem marks, after any edit. */
  function refresh() {
    const problems = pendingProblems(state.pending);
    showProblems(tabBody, problems);
    const changed = changedKeys(state.start, state.pending).length > 0;
    const firstProblem = Object.values(problems)[0];
    const apply = footer.querySelector('[data-action="apply"]');
    const ok = footer.querySelector('[data-action="ok"]');
    apply.disabled = !changed || !!firstProblem;
    apply.title = firstProblem ? S.fixFirstTitle(firstProblem) : (changed ? '' : S.APPLY_NOTHING_TITLE);
    // OK is always there to go back, unless what it would apply can't be.
    ok.disabled = changed && !!firstProblem;
    ok.title = ok.disabled ? S.fixFirstTitle(firstProblem) : S.OK_TITLE;
    banner.hidden = !isBatchRunning();
  }

  function build() {
    kicker = el('div', { class: 'ps-kicker' });
    banner = el('div', { class: 'ps-banner', text: S.BATCH_RUNNING, hidden: true });
    tabBody = el('div', { class: 'ps-tab-body', id: 'project-settings-project-tab' });
    const tabs = el('div', { class: 'ps-tabs', role: 'tablist' }, [
      tabButton(S.TAB_PROJECT, null, true),
      tabButton(S.TAB_CLIP_SETTINGS, S.TAB_CLIP_SETTINGS_SOON, false),
      tabButton(S.TAB_PLAYERS, S.TAB_PLAYERS_SOON, false),
    ]);
    const sections = el('nav', { class: 'ps-sections' }, PROJECT_SECTIONS.map((s) => {
      const link = el('button', { type: 'button', class: 'ps-section-link', text: s.label });
      link.addEventListener('click', () => tabBody.querySelector(`#ps-section-${s.id}`)?.scrollIntoView({ block: 'start', behavior: 'smooth' }));
      return link;
    }));
    footer = el('div', { class: 'ps-footer' }, [
      footerButton('ok', S.OK, 'primary-btn', () => finish('OK')),
      footerButton('cancel', S.CANCEL, '', () => close('Cancel'), S.CANCEL_TITLE),
      footerButton('apply', S.APPLY, '', () => apply('Apply')),
    ]);
    root = el('div', { id: 'project-settings-page', class: 'ps-page', tabIndex: -1, hidden: true }, [
      el('div', { class: 'ps-layout' }, [
        el('aside', { class: 'ps-side' }, [
          kicker,
          el('h2', { class: 'ps-title', text: S.TITLE }),
          sections,
          el('p', { class: 'ps-side-note', text: S.SIDE_NOTE }),
        ]),
        el('div', { class: 'ps-main' }, [tabs, banner, el('div', { class: 'ps-scroll' }, [tabBody])]),
      ]),
      footer,
    ]);
    // Keys typed here stay here: the pages under it (the queue's arrows, the
    // Overviews tab's Ctrl+Z) listen on the document and window.
    root.addEventListener('keydown', (e) => { if (!isSwallowedAppKey(e)) e.stopPropagation(); });
    document.body.append(root);
  }

  function tabButton(label, soonTitle, active) {
    const tab = el('button', {
      type: 'button', class: `ps-tab${active ? ' ps-tab-on' : ''}`, text: label, title: soonTitle || '', disabled: !active,
    });
    tab.setAttribute('role', 'tab');
    tab.setAttribute('aria-selected', active ? 'true' : 'false');
    return tab;
  }

  function footerButton(action, label, extraClass, onClick, title = '') {
    const button = el('button', { type: 'button', class: extraClass, text: label, title });
    button.dataset.action = action;
    button.addEventListener('click', onClick);
    return button;
  }

  /** Sits the page under the menu bar, which stays usable. */
  function placeUnderNavbar() {
    if (root) root.style.top = `${navbar ? navbar.getBoundingClientRect().bottom : 0}px`;
  }

  function isOpen() {
    return !!state;
  }

  function open() {
    // Hidden behind the New UI switch until the rework ships (new_ui.js).
    if (state || !isNewUiOn()) return;
    const { form, block, pcId } = blockForPage();
    if (!form) return; // settings not loaded yet: nothing to start from
    if (!root) build();
    const start = startingValues(form, block, pcId);
    state = { start, pending: structuredClone(start), block, form, pcId, ui: {} };
    const path = getProjectPath();
    kicker.textContent = path ? fileNameOf(path) : S.UNTITLED;
    kicker.title = path || '';
    projectTab.render(tabBody);
    refresh();
    placeUnderNavbar();
    root.hidden = false;
    root.querySelector('.ps-scroll').scrollTop = 0;
    // Focus inside, so keys reach the page's own listener first.
    root.focus();
    setAppShortcutsEnabled(false);
    document.addEventListener('click', onClickOutside, true);
    window.addEventListener('resize', placeUnderNavbar);
    logFrontendEvent(projectSettingsPageOpenedLine(path));
  }

  /** Writes the pending edits into the project. Returns whether anything
   *  changed. */
  function apply(how) {
    if (!state) return false;
    const changed = changedKeys(state.start, state.pending);
    if (changed.length === 0 || Object.keys(pendingProblems(state.pending)).length > 0) return false;
    // Written over the project's block as it is now, so a change made
    // meanwhile (Configuration's, while it still follows into the project)
    // to a setting not edited here is kept.
    const { block: current, pcId } = blockForPage();
    const next = blockWithPending(current, state.start, state.pending, pcId);
    applyBlockFromPage(next);
    state.block = next;
    state.start = structuredClone(state.pending);
    logFrontendEvent(projectSettingsPageAppliedLine(getProjectPath(), how, changed));
    onSettingsInEffectChange();
    refresh();
    return true;
  }

  function finish(how) {
    apply(how);
    close(how);
  }

  /** Hides the page and drops what wasn't applied. */
  function close(how) {
    if (!state) return;
    const dropped = changedKeys(state.start, state.pending).length;
    state = null;
    dismissLeavePrompt('stay');
    root.hidden = true;
    setAppShortcutsEnabled(true);
    document.removeEventListener('click', onClickOutside, true);
    window.removeEventListener('resize', placeUnderNavbar);
    logFrontendEvent(projectSettingsPageClosedLine(getProjectPath(), how, dropped));
  }

  /** See project_settings.js's leaveProjectSettingsPage. */
  async function leave(how) {
    if (!state) return true;
    if (changedKeys(state.start, state.pending).length === 0) {
      close(how);
      return true;
    }
    const answer = await askLeave();
    if (answer === 'stay') return false;
    if (answer === 'apply') finish(`Apply, then ${how}`);
    else close(`Discard, then ${how}`);
    return true;
  }

  /** Apply, Discard or Stay, drawn over the page. Apply is greyed while a
   *  pending value can't be used. */
  function askLeave() {
    dismissLeavePrompt('stay');
    const problem = Object.values(pendingProblems(state.pending))[0];
    return new Promise((resolve) => {
      const answer = (value) => () => dismissLeavePrompt(value);
      const applyBtn = el('button', { type: 'button', class: 'primary-btn', text: S.LEAVE_APPLY, disabled: !!problem, title: problem ? S.fixFirstTitle(problem) : '' });
      applyBtn.addEventListener('click', answer('apply'));
      const discardBtn = el('button', { type: 'button', class: 'danger-btn', text: S.LEAVE_DISCARD });
      discardBtn.addEventListener('click', answer('discard'));
      const stayBtn = el('button', { type: 'button', text: S.LEAVE_STAY });
      stayBtn.addEventListener('click', answer('stay'));
      const box = el('div', { class: 'ps-leave', role: 'dialog' }, [
        el('div', { class: 'ps-leave-box' }, [
          el('h3', { text: S.LEAVE_TITLE }),
          el('p', { text: S.LEAVE_MESSAGE }),
          el('div', { class: 'ps-leave-buttons' }, [applyBtn, discardBtn, stayBtn]),
        ]),
      ]);
      leavePrompt = { box, resolve };
      root.append(box);
      stayBtn.focus();
    });
  }

  function dismissLeavePrompt(answer) {
    if (!leavePrompt) return;
    const { box, resolve } = leavePrompt;
    leavePrompt = null;
    box.remove();
    resolve(answer);
  }

  /**
   * A click on the menu bar while the page is open leaves it: the page
   * closes first (asking about pending edits), then the click goes on to
   * what was clicked. Opening a menu doesn't leave; its items do.
   */
  function onClickOutside(e) {
    const target = e.target.closest?.('button');
    if (!state || !target || !navbar?.contains(target)) return;
    if (target.classList.contains('menu-dropdown-btn') || target.id === MENU_ITEM_ID) return;
    const how = target.dataset.nav || target.id || 'menu bar';
    if (changedKeys(state.start, state.pending).length === 0) {
      close(how);
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    document.querySelectorAll('.menu-dropdown.open').forEach((m) => m.classList.remove('open'));
    leave(how).then((left) => { if (left) target.click(); });
  }

  // File › Project Settings… is there only while the New UI switch is on;
  // turning it off closes the page, dropping edits not applied.
  onNewUiChange((on) => {
    if (on) {
      addMenuItem(open);
    } else {
      close('New UI turned off');
      removeMenuItem();
    }
  });
  // A project opened or closed: the page's edits were for the one that was
  // open (the paths that ask first, leaveProjectSettingsPage, have closed it
  // already), and the capture summary must show the new one's settings.
  onProjectBlockReplaced(() => {
    close('another project opened');
    onSettingsInEffectChange();
  });
  setProjectSettingsPageLeave(leave);
  return { open, isOpen, leave };
}

const MENU_ITEM_ID = 'project-settings-menu-btn';

/** Takes File › Project Settings… out of the menu (the New UI switch off). */
function removeMenuItem() {
  document.querySelector(`#${MENU_ITEM_ID}`)?.remove();
}

/** File › Project Settings…, after Save Session. Made here rather than in
 *  index.html, so the page is one module. */
function addMenuItem(open) {
  const panel = document.querySelector('#file-menu-panel');
  if (!panel || panel.querySelector(`#${MENU_ITEM_ID}`)) return;
  const item = el('button', { type: 'button', class: 'menu-dropdown-item', id: MENU_ITEM_ID, title: S.MENU_TITLE }, [
    el('span', { text: S.MENU_ITEM }),
  ]);
  item.addEventListener('click', open);
  const after = panel.querySelector('#save-project-btn');
  if (after) after.after(item);
  else panel.append(item);
}
