// project_settings_project_tab.js
// The Project tab of the Project settings page (#1085, build plan step 6):
// Picture, Clip length, Clip names, Folders and Install, drawn from the
// page's pending values. It never writes the project: each edit goes to the
// page (onEdit), which holds it until OK or Apply. The rules are in
// project_settings_page_logic.js.

import {
  CAPTURE_MODES, SIZE_CHOICES, RATE_CHOICES, sizeChoiceOf, rateChoiceOf, rateKeyFor, rateOf, codecKeyFor,
  templateOrDefault, killGapsWith, withFolder, withoutFolder, sameDriveLines, installState,
} from './project_settings_page_logic.js';
import { DEFAULT_TEMPLATE, PLACEHOLDERS } from './clip_name.js';
import { insertIntoTemplate } from './clip_name_ui.js';
import { placeholderGroups, placeholderDescription } from './placeholder_catalogue.js';
import { renderPlaceholderChips } from './placeholder_chips.js';
import { pickPath } from './pick_path.js';
import { calculateExportPoolSpace } from './ipc_bridge.js';
import { BYTES_PER_GB } from './byte_size.js';
import { madeWithOf } from './project_settings_logic.js';
import { STRINGS } from './strings.js';

const S = STRINGS.PROJECT_SETTINGS_PAGE;

const MODE_LABELS = {
  frame_sequence: S.MODE_FRAMES, direct_to_video: S.MODE_VIDEO, obs: S.MODE_OBS, agr: S.MODE_AGR,
};
const SIZE_LABELS = { '1080p': S.SIZE_1080P, '1440p': S.SIZE_1440P, '4k': S.SIZE_4K };

/** The sections, in page order, for the side list and the cards. */
export const PROJECT_SECTIONS = Object.freeze([
  { id: 'picture', label: S.PICTURE },
  { id: 'clip-length', label: S.CLIP_LENGTH },
  { id: 'clip-names', label: S.CLIP_NAMES },
  { id: 'folders', label: S.FOLDERS },
  { id: 'install', label: S.INSTALL },
]);

/** An element with `props` (class, text, title, and any other property)
 *  and `children`. */
function el(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (value === undefined || value === null) continue;
    if (key === 'class') node.className = value;
    else if (key === 'text') node.textContent = value;
    else if (key === 'dataset') Object.assign(node.dataset, value);
    else node[key] = value;
  }
  node.append(...children.filter(Boolean));
  return node;
}

/** A row: its label and hint on the left, `controls` on the right. */
function row(label, hint, controls, { id } = {}) {
  return el('div', { class: 'ps-row', dataset: id ? { row: id } : {} }, [
    el('div', { class: 'ps-row-label' }, [
      el('div', { class: 'ps-row-title', text: label }),
      hint ? el('div', { class: 'ps-row-hint', text: hint }) : null,
    ]),
    el('div', { class: 'ps-row-controls' }, controls),
  ]);
}

/** A card for one section. */
function card(id, title, sub, rows) {
  return el('section', { class: 'ps-card', id: `ps-section-${id}` }, [
    el('h3', { class: 'ps-card-title' }, [
      document.createTextNode(title),
      sub ? el('span', { class: 'ps-card-sub', text: sub }) : null,
    ]),
    ...rows,
  ]);
}

/** A row of chips, one picked. `choices` are `{ value, label, title }`. */
function chips(name, choices, picked, onPick) {
  return el('div', { class: 'ps-chips', role: 'group', dataset: { chips: name } }, choices.map((c) => {
    const on = c.value === picked;
    const chip = el('button', {
      type: 'button', class: `ps-chip${on ? ' ps-chip-on' : ''}`, text: c.label, title: c.title || '',
      dataset: { value: String(c.value) },
    });
    chip.setAttribute('aria-pressed', on ? 'true' : 'false');
    chip.addEventListener('click', () => { if (!on) onPick(c.value); });
    return chip;
  }));
}

/** A number box: `onValue(number|NaN)` on every keystroke. */
function numberBox(name, value, onValue, { step = '1', width = '84px', title } = {}) {
  const input = el('input', {
    type: 'number', class: 'ps-number', step, value: value === undefined || value === null ? '' : String(value),
    title: title || '', dataset: { field: name },
  });
  input.style.width = width;
  input.addEventListener('input', () => onValue(input.value.trim() === '' ? NaN : Number(input.value)));
  return input;
}

const unit = (text) => el('span', { class: 'ps-unit', text });

/**
 * @param {object} page  `getState()` the page's `{ pending, block, form, ui }`
 *   (`ui` holds which Custom boxes are open, kept across redraws);
 *   `onEdit(changes, { redraw })` puts `changes` (settings-file keys) into
 *   the pending values, redrawing the tab when a control appears or goes.
 */
export function createProjectTab({ getState, onEdit }) {
  // Which free-space read is current, so a slow one never overwrites a newer.
  let spaceRead = 0;

  function pictureCard() {
    const { pending, ui } = getState();
    const mode = pending.capture_mode;
    const rows = [
      row(S.CAPTURE_WITH, S.CAPTURE_WITH_HINT, [
        chips('capture_mode', CAPTURE_MODES.map((m) => ({ value: m, label: MODE_LABELS[m] })), mode,
          (m) => { ui.customRate = false; onEdit({ capture_mode: m }, { redraw: true }); }),
      ]),
      sizeRow(pending, ui),
      rateRow(pending, ui, mode),
      finishRow(pending, mode),
    ];
    return card('picture', S.PICTURE, S.PICTURE_SUB, rows);
  }

  function sizeRow(pending, ui) {
    const picked = ui.customSize ? 'custom' : sizeChoiceOf(pending.resolution_width, pending.resolution_height);
    const choices = [...SIZE_CHOICES.map((c) => ({ value: c.id, label: SIZE_LABELS[c.id], title: `${c.width}×${c.height}` })),
      { value: 'custom', label: S.CUSTOM }];
    const controls = [chips('size', choices, picked, (id) => {
      const choice = SIZE_CHOICES.find((c) => c.id === id);
      ui.customSize = !choice;
      onEdit(choice ? { resolution_width: choice.width, resolution_height: choice.height } : {}, { redraw: true });
    })];
    if (picked === 'custom') {
      controls.push(el('div', { class: 'ps-inline' }, [
        unit(S.WIDTH), numberBox('resolution_width', pending.resolution_width, (v) => onEdit({ resolution_width: v })),
        unit(S.BY),
        unit(S.HEIGHT), numberBox('resolution_height', pending.resolution_height, (v) => onEdit({ resolution_height: v })),
      ]));
    }
    return row(S.SIZE, S.SIZE_HINT, controls, { id: 'resolution_width' });
  }

  function rateRow(pending, ui, mode) {
    const key = rateKeyFor(mode);
    const rate = rateOf(pending, mode);
    const picked = ui.customRate ? 'custom' : rateChoiceOf(rate);
    const choices = [...RATE_CHOICES.map((fps) => ({ value: fps, label: S.fps(fps) })), { value: 'custom', label: S.CUSTOM }];
    const controls = [chips('rate', choices, picked, (fps) => {
      ui.customRate = fps === 'custom';
      onEdit(fps === 'custom' ? {} : { [key]: fps }, { redraw: true });
    })];
    if (picked === 'custom') {
      controls.push(el('div', { class: 'ps-inline' }, [
        numberBox(key, rate, (v) => onEdit({ [key]: v })), unit(S.FPS_UNIT),
      ]));
    }
    // AGR records positions, not frames; its own rate is the one that counts (R14).
    if (mode === 'agr') return row(S.AGR_RATE, S.AGR_RATE_HINT, controls, { id: key });
    return row(S.SMOOTHNESS, mode === 'obs' ? S.SMOOTHNESS_OBS_HINT : S.SMOOTHNESS_HINT, controls, { id: key });
  }

  function finishRow(pending, mode) {
    const codecKey = codecKeyFor(mode);
    if (!codecKey) return row(S.FINISHED, S.AGR_NOTHING_TO_FINISH, []);
    const finishing = !!pending.finish_clips_after_batch;
    const controls = [chips('finish', [
      { value: false, label: S.FINISH_LEAVE },
      { value: true, label: S.FINISH_AUTO },
    ], finishing, (on) => onEdit({ finish_clips_after_batch: on }, { redraw: true }))];
    const select = codecSelect(codecKey, pending[codecKey]);
    // One precondition away from mattering: dimmed, not hidden.
    select.disabled = !finishing;
    controls.push(el('div', { class: 'ps-inline' }, [unit(S.SAVED_AS), select]));
    if (mode === 'obs') controls.push(el('div', { class: 'ps-note', text: S.OBS_KEEP_NOTE }));
    return row(S.FINISHED, S.FINISHED_HINT, controls);
  }

  /** The codec choices for `codecKey`, copied from Configuration's own
   *  select of that mode, so the two lists can never differ. */
  function codecSelect(codecKey, value) {
    const source = document.querySelector(`#config-${codecKey.replace(/_/g, '-')}`);
    const select = el('select', { class: 'ps-select', title: S.SAVED_AS_TITLE, dataset: { field: codecKey } });
    for (const option of source?.options || []) {
      select.append(el('option', {
        value: option.value,
        text: option.value === 'render_tab' ? S.RENDER_TAB_CODEC : option.textContent,
      }));
    }
    select.value = value;
    select.addEventListener('change', () => onEdit({ [codecKey]: select.value }));
    return select;
  }

  function clipLengthCard() {
    const { pending } = getState();
    const line = el('div', { class: 'ps-note', dataset: { lengthLine: '' } });
    const refreshLine = () => { line.textContent = S.lengthLine(lengthText(getState().pending.record_start_lead), lengthText(getState().pending.record_stop_trail)); };
    const seconds = (key) => el('div', { class: 'ps-inline' }, [
      numberBox(key, pending[key], (v) => { onEdit({ [key]: v }); refreshLine(); }, { step: '0.1' }), unit(S.SECONDS_UNIT),
    ]);
    refreshLine();
    return card('clip-length', S.CLIP_LENGTH, S.CLIP_LENGTH_SUB, [
      row(S.BEFORE, S.BEFORE_HINT, [seconds('record_start_lead')], { id: 'record_start_lead' }),
      row(S.AFTER, S.AFTER_HINT, [seconds('record_stop_trail'), line], { id: 'record_stop_trail' }),
      gapsRow(pending),
    ]);
  }

  function gapsRow(pending) {
    const gaps = pending.kill_gaps || {};
    const controls = [chips('kill_gaps', [
      { value: 'keep', label: S.GAPS_KEEP },
      { value: 'fast_forward', label: S.GAPS_FAST },
    ], gaps.mode, (m) => onEdit({ kill_gaps: killGapsWith(getState().pending.kill_gaps, { mode: m }) }, { redraw: true }))];
    if (gaps.mode === 'fast_forward') {
      const edit = (field) => (v) => onEdit({ kill_gaps: killGapsWith(getState().pending.kill_gaps, { [field]: v }) });
      controls.push(el('div', { class: 'ps-inline' }, [
        unit(S.GAPS_OVER), numberBox('kill_gap_seconds', gaps.over_seconds, edit('over_seconds'), { step: '0.5', width: '64px' }),
        unit(S.SECONDS_UNIT), unit(S.GAPS_AT),
        numberBox('kill_gap_speed', gaps.speed, edit('speed'), { width: '56px' }), unit(S.SPEED_UNIT),
      ]));
    }
    return row(S.GAPS, S.GAPS_HINT, controls, { id: 'kill_gaps' });
  }

  function clipNamesCard() {
    const { pending } = getState();
    const input = el('input', {
      type: 'text', class: 'ps-text ps-mono', value: pending.clip_name_template, spellcheck: false,
      autocomplete: 'off', dataset: { field: 'clip_name_template' },
    });
    input.addEventListener('input', () => onEdit({ clip_name_template: templateOrDefault(input.value) }));
    // A box left blank reads as the default, so it shows the default.
    input.addEventListener('change', () => { if (!input.value.trim()) input.value = DEFAULT_TEMPLATE; });
    const reset = el('button', { type: 'button', class: 'ps-link-btn', text: S.DEFAULT, title: S.defaultTitle(DEFAULT_TEMPLATE) });
    reset.addEventListener('click', () => {
      input.value = DEFAULT_TEMPLATE;
      onEdit({ clip_name_template: DEFAULT_TEMPLATE });
    });
    const box = el('div', { class: 'placeholder-chips ps-placeholders' });
    renderPlaceholderChips(box, {
      groups: placeholderGroups('clip'),
      titleFor: (name) => placeholderDescription('clip', name),
      fallbackNames: PLACEHOLDERS,
      onInsert: (text) => insertIntoTemplate(input, text),
    });
    return card('clip-names', S.CLIP_NAMES, null, [
      row(S.NAME_LIKE, S.NAME_HINT, [el('div', { class: 'ps-inline ps-grow' }, [input, reset])], { id: 'clip_name_template' }),
      row(S.PLACEHOLDERS, S.PLACEHOLDERS_HINT, [box]),
    ]);
  }

  function foldersCard() {
    const { pending } = getState();
    return card('folders', S.FOLDERS, S.FOLDERS_SUB, [
      folderRow('target_drives', S.CAPTURE_FOLDERS, S.CAPTURE_FOLDERS_HINT, pending.target_drives,
        { field: 'capture.output_folder', title: STRINGS.MAIN.SELECT_CAPTURE_OUTPUT_DIR_TITLE, showSpace: true }),
      folderRow('render_export_dirs', S.EXPORT_FOLDERS, S.EXPORT_FOLDERS_HINT, pending.render_export_dirs,
        { field: 'render.export_folder', title: STRINGS.MAIN.SELECT_RENDER_EXPORT_DIR_TITLE }),
    ]);
  }

  function folderRow(key, label, hint, folders, { field, title, showSpace = false }) {
    const list = folders || [];
    const folderChips = list.map((path, i) => {
      const remove = el('button', { type: 'button', class: 'ps-chip-x', text: S.REMOVE_MARK, title: S.removeFolderTitle(path) });
      remove.addEventListener('click', () => onEdit({ [key]: withoutFolder(getState().pending[key], i) }, { redraw: true }));
      return el('span', { class: 'ps-folder-chip', title: path }, [el('span', { text: path }), remove]);
    });
    const add = el('button', { type: 'button', class: 'ps-link-btn', text: S.ADD_FOLDER });
    add.addEventListener('click', async () => {
      let picked = null;
      try {
        picked = await pickPath({ field, directory: true, multiple: false, title });
      } catch (err) {
        console.error('Project settings: folder picker failed:', err);
      }
      // The page may have closed while the dialog was up.
      if (typeof picked === 'string' && getState()) onEdit({ [key]: withFolder(getState().pending[key], picked) }, { redraw: true });
    });
    const controls = [el('div', { class: 'ps-folder-chips' }, [
      ...(folderChips.length ? folderChips : [el('span', { class: 'ps-muted', text: S.NO_FOLDERS })]),
      add,
    ])];
    const warnings = sameDriveLines(list);
    if (warnings.length) controls.push(el('div', { class: 'ps-warn', text: warnings.join('\n') }));
    if (showSpace && list.length) controls.push(freeSpaceLine(list));
    return row(label, hint, controls, { id: key });
  }

  /** "N GB free" over `folders`, filled in when the backend answers. */
  function freeSpaceLine(folders) {
    const line = el('div', { class: 'ps-note', dataset: { freeSpace: '' } });
    const read = ++spaceRead;
    calculateExportPoolSpace(folders)
      .then((bytes) => { if (read === spaceRead) line.textContent = S.freeSpace((bytes / BYTES_PER_GB).toFixed(1)); })
      // ipc_bridge.js logged the error; the line says the space isn't known.
      .catch(() => { if (read === spaceRead) line.textContent = S.FREE_UNKNOWN; });
    return line;
  }

  function installCard() {
    const { block, form } = getState();
    const madeWith = block?.install?.madeWith || null;
    const state = installState(madeWith, form?.hl_path);
    const controls = [];
    if (state === 'none') {
      controls.push(el('span', { class: 'ps-muted', text: S.INSTALL_NONE }));
    } else {
      controls.push(el('span', { class: 'ps-install-name', text: madeWith.name || madeWith.hlExe, title: madeWith.hlExe || '' }));
      const picked = madeWithOf(form?.hl_path)?.name || STRINGS.PROJECT_SETTINGS.NO_INSTALL;
      controls.push(el('span', {
        class: `ps-state ${state === 'picked' ? 'ps-state-ok' : 'ps-state-warn'}`,
        text: state === 'picked' ? S.INSTALL_PICKED : S.INSTALL_OTHER,
        title: state === 'picked' ? '' : S.installOtherTitle(picked),
      }));
    }
    return card('install', S.INSTALL, null, [row(S.INSTALL, S.INSTALL_HINT, controls)]);
  }

  /** Draws the tab into `container`, replacing what was there. */
  function render(container) {
    container.replaceChildren(pictureCard(), clipLengthCard(), clipNamesCard(), foldersCard(), installCard());
  }

  return { render, showProblems };
}

/**
 * Marks each row in `container` that `problems` (pendingProblems' `{ key:
 * message }`) names: its boxes red and the message under it. Clears the
 * marks of rows that are fine now. Run on every edit, without a redraw, so
 * a box being typed in keeps its focus.
 */
export function showProblems(container, problems) {
  container.querySelectorAll('.ps-problem').forEach((line) => line.remove());
  container.querySelectorAll('.ps-invalid').forEach((input) => input.classList.remove('ps-invalid'));
  for (const [key, message] of Object.entries(problems)) {
    const target = container.querySelector(`[data-row="${key}"] .ps-row-controls`);
    if (!target) continue;
    target.querySelectorAll('input').forEach((input) => input.classList.add('ps-invalid'));
    target.append(el('div', { class: 'ps-problem', role: 'alert', text: message }));
  }
}

/** A seconds value for the clip-length line: as typed, or ? while blank. */
function lengthText(value) {
  return Number.isFinite(value) ? String(value) : '?';
}
