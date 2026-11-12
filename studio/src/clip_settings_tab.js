// clip_settings_tab.js
// The Clip settings tab of the Project settings page (#1085, build plan
// step 8): a preset to start from, the rows (clip_settings_rows.js), Your
// own commands and At each clip, drawn from the page's pending values. Like
// the Project tab it never writes the project: each edit goes to the page
// (onEdit), which holds it until OK or Apply. The rows are an editor over
// the project's Initial Commands lines; the rules are clip_settings_logic.js's.

import { CLIP_SETTINGS_SECTIONS, ROW_CVARS } from './clip_settings_rows.js';
import {
  rowValue, withRowValue, ownEntries, withLineAt, withoutLineAt, withRowLinesMerged,
  DEFAULT_FOV, presetPick, numberRowValue, parseSetting, clipRowKey,
} from './clip_settings_logic.js';
import { profileState } from './command_profiles.js';
import { isBuiltinName } from './builtin_profiles.js';
import { attachCommandSuggest, tierNote, typedName } from './command_suggest.js';
import { el, row, card, chips } from './project_settings_dom.js';
import { STRINGS } from './strings.js';

const S = STRINGS.CLIP_SETTINGS;

/** The sections, in page order, for the side list and the cards. */
export const CLIP_SECTIONS = Object.freeze([
  { id: 'start-from', label: S.SECTIONS['start-from'] },
  ...CLIP_SETTINGS_SECTIONS.map((s) => ({ id: s.id, label: S.SECTIONS[s.id] })),
  { id: 'own-commands', label: S.SECTIONS['own-commands'] },
]);

/** Seek Between Clips, Configuration's app setting: on, the hook runs both
 *  lists past the demo filter, which changes what a line's note says. */
const seeks = () => document.querySelector('#config-hook-runs-batch')?.checked ?? true;

/** The value a "Not set" choice carries: no line for the row. */
const UNSET = 'unset';

/** The note under a command, from command_suggest.js's tiers, or null.
 *  The page is the New UI's, so the project always sets the field of view
 *  (#1085): a `mirv_fov <n>` line here is refused. */
function noteFor(command, scheduled) {
  const name = typedName(command) ?? String(command).trim().split(/\s+/)[0];
  const line = typedName(command) === null ? String(command) : null;
  const note = name ? tierNote(name.toLowerCase(), { scheduled, seeks: seeks(), projectFov: true, line }) : null;
  return note ? note.text : '';
}

/**
 * @param {object} page  `getState()` the page's `{ pending, ui }`;
 *   `onEdit(changes, { redraw })` as the Project tab's; `getPresets()` the
 *   presets to pick from (built in first, command_profiles_ui.js);
 *   `savePreset(name, lists)` saves one on this PC and returns its name, or
 *   null; `goTo(tab, sectionId)` shows another tab's section;
 *   `onPresetSaved(name, lists)` logs and toasts a saved preset.
 */
export function createClipSettingsTab({ getState, onEdit, getPresets, savePreset, goTo, onPresetSaved = () => {} }) {
  const lines = () => getState().pending.init_commands || [];
  const setLines = (next, redraw = false) => onEdit({ init_commands: next }, { redraw });

  function startFromCard() {
    const { pending, ui } = getState();
    const presets = getPresets();
    const state = profileState(presets, pending.command_profile_active, pending);
    const choices = [
      ...presets.map((p) => ({
        value: p.name,
        label: state.edited && p.name === state.name ? S.presetEdited(p.name) : p.name,
        title: p.builtin ? S.BUILTIN_TITLE : '',
      })),
      { value: '', label: S.NO_PRESET, title: S.NO_PRESET_TITLE },
    ];
    const controls = [chips('preset', choices, state.name, (name) => {
      onEdit(presetPick(presets.find((p) => p.name === name) || null), { redraw: true });
    })];
    controls.push(ui.saveAsOpen ? saveAsBox() : saveAsButton());
    controls.push(el('div', { class: 'ps-note', text: S.MANAGE_NOTE }));
    return card('start-from', S.SECTIONS['start-from'], S.START_FROM_SUB, [
      row(S.PRESET, S.PRESET_HINT, controls, { id: 'clip-preset' }),
    ]);
  }

  function saveAsButton() {
    const button = el('button', { type: 'button', class: 'ps-link-btn', text: S.SAVE_AS_NEW, title: S.SAVE_AS_NEW_TITLE });
    button.addEventListener('click', () => {
      getState().ui.saveAsOpen = true;
      onEdit({}, { redraw: true });
      document.querySelector('[data-field="preset-name"]')?.focus();
    });
    return button;
  }

  function saveAsBox() {
    const input = el('input', {
      type: 'text', class: 'ps-text', placeholder: S.SAVE_AS_NAME_PLACEHOLDER, spellcheck: false,
      autocomplete: 'off', dataset: { field: 'preset-name' },
    });
    const problem = el('div', { class: 'ps-warn', hidden: true });
    const save = el('button', { type: 'button', class: 'primary-btn', text: S.SAVE, dataset: { action: 'save-preset' } });
    const cancel = el('button', { type: 'button', text: S.CANCEL_SAVE });
    const close = () => {
      getState().ui.saveAsOpen = false;
      onEdit({}, { redraw: true });
    };
    const trySave = () => {
      const name = input.value.trim();
      if (!name) return;
      const presets = getPresets();
      if (isBuiltinName(presets, name)) {
        problem.textContent = S.presetNameTaken(name);
        problem.hidden = false;
        return;
      }
      const { pending, ui } = getState();
      const lists = { init_commands: pending.init_commands, custom_commands: pending.custom_commands };
      const saved = savePreset(name, lists);
      if (!saved) return;
      ui.saveAsOpen = false;
      onPresetSaved(saved, lists);
      onEdit({ command_profile_active: saved }, { redraw: true });
    };
    save.addEventListener('click', trySave);
    cancel.addEventListener('click', close);
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') trySave();
      else if (e.key === 'Escape') close();
    });
    return el('div', { class: 'ps-inline ps-grow' }, [input, save, cancel, problem]);
  }

  /** One row's controls, by its kind. */
  function rowControls(r) {
    const value = r.cvar ? rowValue(lines(), r.cvar) : null;
    if (r.kind === 'toggle') return [toggleChips(r, value)];
    if (r.kind === 'select') return [selectBox(r, value)];
    if (r.kind === 'number') return [numberInput(r, value)];
    if (r.kind === 'picture') {
      const { pending } = getState();
      const fov = pending.capture_fov ?? DEFAULT_FOV;
      const link = el('button', { type: 'button', class: 'ps-link-btn', text: S.PICTURE_LINK, title: S.PICTURE_LINK_TITLE });
      link.addEventListener('click', () => goTo('project', 'picture'));
      return [el('span', { class: 'ps-install-name', text: S.pictureValue(fov) }), link];
    }
    return [el('span', { class: 'ps-muted', text: S.SPRAYS_STATE, title: S.SPRAYS_TITLE })];
  }

  /** Not set, Off, On; and the typed value when a line holds another. */
  function toggleChips(r, value) {
    const choices = [
      { value: UNSET, label: S.NOT_SET, title: S.NOT_SET_TITLE },
      { value: '0', label: S.OFF },
      { value: '1', label: S.ON },
    ];
    if (value !== null && !r.values.includes(value)) choices.push({ value, label: S.typedValue(value) });
    return chips(r.cvar, choices, value ?? UNSET, (v) => setLines(withRowValue(lines(), r.cvar, v === UNSET ? null : v), true));
  }

  function selectBox(r, value) {
    const select = el('select', { class: 'ps-select', dataset: { field: r.cvar } });
    select.append(el('option', { value: UNSET, text: S.NOT_SET, title: S.NOT_SET_TITLE }));
    r.values.forEach((v, i) => select.append(el('option', { value: v, text: S.CHOICES[r.id][i] })));
    if (value !== null && !r.values.includes(value)) select.append(el('option', { value, text: S.typedValue(value) }));
    select.value = value ?? UNSET;
    select.addEventListener('change', () => setLines(withRowValue(lines(), r.cvar, select.value === UNSET ? null : select.value)));
    return select;
  }

  function numberInput(r, value) {
    const input = el('input', {
      type: 'text', class: 'ps-number', value: value ?? '', placeholder: S.NOT_SET, title: S.NOT_SET_TITLE,
      inputMode: 'decimal', spellcheck: false, autocomplete: 'off', dataset: { field: r.cvar },
    });
    input.style.width = '84px';
    input.addEventListener('input', () => setLines(withRowValue(lines(), r.cvar, numberRowValue(input.value))));
    return input;
  }

  function settingRow(r) {
    const [label, hint] = S.ROWS[r.id];
    const controls = rowControls(r);
    if (r.when === 'load') controls.push(el('span', { class: 'ps-state ps-state-warn', text: S.AT_LOAD, title: S.AT_LOAD_TITLE }));
    const node = row(label, hint, controls, { id: clipRowKey(r) });
    if (r.cvar) node.title = r.cvar;
    return node;
  }

  function sectionCard(section) {
    return card(section.id, S.SECTIONS[section.id], S.SECTION_SUBS[section.id] || null, section.rows.map(settingRow));
  }

  function ownCommandsCard() {
    const order = el('div', { class: 'ps-chips' }, S.WHICH_WINS_ORDER.map((label, i) => el('span', {
      class: 'ps-chip', text: `${i + 1} · ${label}`,
    })));
    return card('own-commands', S.SECTIONS['own-commands'], S.OWN_COMMANDS_SUB, [
      row(S.WHICH_WINS, S.WHICH_WINS_HINT, [order]),
      row(S.COMMANDS, S.COMMANDS_HINT, [ownList()], { id: 'init_commands' }),
      row(S.AT_EACH_CLIP, S.AT_EACH_CLIP_HINT, [scheduledList()], { id: 'custom_commands' }),
    ]);
  }

  /** A command box with its type-ahead and the note under it. */
  function commandBox(text, scheduled, field, onText) {
    const input = el('input', {
      type: 'text', class: 'ps-text ps-mono', value: text, spellcheck: false, autocomplete: 'off', dataset: { field },
    });
    const note = el('div', { class: 'ps-warn', text: noteFor(text, scheduled) });
    note.hidden = !note.textContent;
    attachCommandSuggest(input, { scheduled, seeks, projectFov: () => true });
    input.addEventListener('input', () => {
      onText(input.value);
      note.textContent = noteFor(input.value, scheduled);
      note.hidden = !note.textContent;
    });
    return { input, note };
  }

  function removeButton(onRemove) {
    const button = el('button', { type: 'button', class: 'ps-chip-x', text: STRINGS.PROJECT_SETTINGS_PAGE.REMOVE_MARK, title: S.REMOVE_COMMAND_TITLE });
    button.addEventListener('click', onRemove);
    return button;
  }

  function addButton(onAdd) {
    const button = el('button', { type: 'button', class: 'ps-link-btn', text: S.ADD_COMMAND });
    button.addEventListener('click', onAdd);
    return button;
  }

  function ownList() {
    const entries = ownEntries(lines());
    const items = entries.map(({ index, text }) => {
      const { input, note } = commandBox(text, false, `own-${index}`, (value) => setLines(withLineAt(lines(), index, value)));
      // A line that now sets a row moves into it.
      input.addEventListener('change', () => {
        if (ROW_CVARS.includes(parseSetting(input.value)?.name)) setLines(withRowLinesMerged(lines()), true);
      });
      return el('div', { class: 'ps-command' }, [
        el('div', { class: 'ps-inline ps-grow' }, [input, removeButton(() => setLines(withoutLineAt(lines(), index), true))]),
        note,
      ]);
    });
    return el('div', { class: 'ps-commands' }, [
      ...(items.length ? items : [el('span', { class: 'ps-muted', text: S.NO_COMMANDS })]),
      addButton(() => {
        getState().ui.focusLast = 'own';
        setLines([...lines(), ''], true);
      }),
    ]);
  }

  function scheduledList() {
    const list = () => getState().pending.custom_commands || [];
    const setList = (next, redraw = false) => onEdit({ custom_commands: next }, { redraw });
    const change = (i, fields) => setList(list().map((c, j) => (j === i ? { ...c, ...fields } : c)));
    const items = list().map((c, i) => {
      const { input, note } = commandBox(c.command, true, `scheduled-${i}`, (value) => change(i, { command: value }));
      const relation = el('select', { class: 'ps-select', dataset: { field: `scheduled-relation-${i}` } }, [
        el('option', { value: 'Before', text: S.BEFORE }),
        el('option', { value: 'After', text: S.AFTER }),
      ]);
      relation.value = c.relation === 'After' ? 'After' : 'Before';
      relation.addEventListener('change', () => change(i, { relation: relation.value }));
      const seconds = el('input', {
        type: 'number', class: 'ps-number', step: '0.1', min: '0', dataset: { field: `scheduled-seconds-${i}` },
        value: Number.isFinite(c.offset_seconds) ? String(c.offset_seconds) : '',
      });
      seconds.style.width = '64px';
      seconds.addEventListener('input', () => change(i, { offset_seconds: seconds.value.trim() === '' ? NaN : Number(seconds.value) }));
      return el('div', { class: 'ps-command' }, [
        el('div', { class: 'ps-inline ps-grow' }, [
          input, relation, seconds, el('span', { class: 'ps-unit', text: S.SECONDS }),
          removeButton(() => setList(list().filter((_, j) => j !== i), true)),
        ]),
        note,
      ]);
    });
    return el('div', { class: 'ps-commands' }, [
      ...(items.length ? items : [el('span', { class: 'ps-muted', text: S.NO_COMMANDS })]),
      addButton(() => {
        getState().ui.focusLast = 'scheduled';
        setList([...list(), { command: '', relation: 'Before', offset_seconds: 2.0 }], true);
      }),
    ]);
  }

  /** Draws the tab into `container`, replacing what was there. */
  function render(container) {
    container.replaceChildren(startFromCard(), ...CLIP_SETTINGS_SECTIONS.map(sectionCard), ownCommandsCard());
    const { ui } = getState();
    if (ui.focusLast) {
      const boxes = container.querySelectorAll(`[data-field^="${ui.focusLast === 'own' ? 'own-' : 'scheduled-'}"]`);
      [...boxes].filter((b) => b.tagName === 'INPUT' && b.type === 'text').pop()?.focus();
      ui.focusLast = null;
    }
  }

  return { render };
}
