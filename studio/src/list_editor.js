// Shared editable row-list widget.
//
// Backs Capture Locations (also Render Studio's scan input), Render
// Studio's Export Drives, Init Commands, and Custom Commands so all four
// lists share one add/edit/remove/reorder implementation instead of four
// hand-rolled ones.
// Items are either plain values (`fields[0].primitive: true`, e.g. a path
// string) or objects keyed by each field's `key`.

import { STRINGS } from './strings.js';

function primaryValue(item, fields) {
  return fields[0].primitive ? item : item[fields[0].key];
}

function fieldValue(item, field) {
  return field.primitive ? item : item[field.key];
}

function setFieldValue(items, idx, field, value) {
  if (field.primitive) items[idx] = value;
  else items[idx][field.key] = value;
}

// Inline SVG (not the 🗑 emoji) so the icon actually inherits `currentColor` —
// WebView2 falls back to a monochrome glyph for 🗑 that ignores CSS `color`,
// which reads as a pause icon in light-grey-on-dark-grey.
export const TRASH_ICON_SVG = `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"></path><path d="M10 11v6"></path><path d="M14 11v6"></path><path d="M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2"></path></svg>`;

/**
 * @param {Object} opts
 * @param {Element} opts.container - element rows are rendered into
 * @param {() => Array} opts.getItems - returns the live backing array (read fresh on every render, so it stays valid across settings reloads that reassign the array)
 * @param {Array<Object>} opts.fields - field specs; fields[0] is the primary field (used for uniqueness + the browse button). Each entry: `{ key, type: 'text'|'select'|'number', primitive?, options?, placeholder?, width?, step?, min? }`
 * @param {boolean} [opts.unique] - reject add()/browse() when the primary field's value already exists elsewhere in the list
 * @param {() => void} [opts.onChange] - called after any add/edit/remove/reorder
 * @param {() => Promise<string|null>} [opts.browse] - opens a native picker for the primary field; adds a per-row 📁 button when set
 */
export function createListEditor({ container, getItems, fields, unique = false, onChange, browse }) {
  function notify() {
    onChange?.();
  }

  function hasValue(value, exceptIdx = -1) {
    return getItems().some((it, i) => i !== exceptIdx && primaryValue(it, fields) === value);
  }

  function buildRow(items, idx) {
    const item = items[idx];
    const row = document.createElement('div');
    row.className = 'input-row list-editor-row';

    fields.forEach((field) => {
      const value = fieldValue(item, field);
      let input;
      if (field.type === 'select') {
        input = document.createElement('select');
        (field.options || []).forEach((opt) => {
          const o = document.createElement('option');
          o.value = opt;
          o.textContent = opt;
          if (value === opt) o.selected = true;
          input.appendChild(o);
        });
        input.addEventListener('change', (e) => {
          setFieldValue(items, idx, field, e.target.value);
          notify();
        });
      } else {
        input = document.createElement('input');
        input.type = field.type === 'number' ? 'number' : 'text';
        if (field.step != null) input.step = String(field.step);
        if (field.min != null) input.min = String(field.min);
        if (field.width) input.style.width = field.width;
        else input.style.flex = '1';
        if (field.placeholder) input.placeholder = field.placeholder;
        input.value = value ?? '';
        // Keep the in-memory item updated every keystroke, but only notify
        // (-> settings autosave-to-disk) on 'change' (blur/Enter) — matches
        // detail_pane.js's notes-field split, avoiding a full settings write
        // per character while composing a value, notably free-text Custom
        // Commands / Init Commands entries.
        input.addEventListener('input', (e) => {
          const v = field.type === 'number' ? (parseFloat(e.target.value) || 0) : e.target.value;
          setFieldValue(items, idx, field, v);
        });
        input.addEventListener('change', () => notify());
      }
      input.className = 'list-editor-field';
      row.appendChild(input);
    });

    if (browse) {
      const browseBtn = document.createElement('button');
      browseBtn.type = 'button';
      browseBtn.className = 'list-editor-browse-btn';
      browseBtn.textContent = '📁';
      browseBtn.title = STRINGS.LIST_EDITOR.BROWSE_TITLE;
      browseBtn.addEventListener('click', async () => {
        const selected = await browse();
        if (!selected) return;
        if (unique && hasValue(selected, idx)) return;
        setFieldValue(items, idx, fields[0], selected);
        notify();
        render();
      });
      row.appendChild(browseBtn);
    }

    const upBtn = document.createElement('button');
    upBtn.type = 'button';
    upBtn.className = 'list-editor-move-btn';
    upBtn.textContent = '▲';
    upBtn.title = STRINGS.LIST_EDITOR.MOVE_UP_TITLE;
    upBtn.disabled = idx === 0;
    upBtn.addEventListener('click', () => {
      if (idx === 0) return;
      [items[idx - 1], items[idx]] = [items[idx], items[idx - 1]];
      notify();
      render();
    });
    row.appendChild(upBtn);

    const downBtn = document.createElement('button');
    downBtn.type = 'button';
    downBtn.className = 'list-editor-move-btn';
    downBtn.textContent = '▼';
    downBtn.title = STRINGS.LIST_EDITOR.MOVE_DOWN_TITLE;
    downBtn.disabled = idx === items.length - 1;
    downBtn.addEventListener('click', () => {
      if (idx === items.length - 1) return;
      [items[idx], items[idx + 1]] = [items[idx + 1], items[idx]];
      notify();
      render();
    });
    row.appendChild(downBtn);

    const removeBtn = document.createElement('button');
    removeBtn.type = 'button';
    removeBtn.className = 'list-editor-remove-btn';
    removeBtn.innerHTML = TRASH_ICON_SVG;
    removeBtn.title = STRINGS.LIST_EDITOR.REMOVE_TITLE;
    removeBtn.setAttribute('aria-label', STRINGS.LIST_EDITOR.REMOVE_ARIA_LABEL);
    removeBtn.addEventListener('click', () => {
      items.splice(idx, 1);
      notify();
      render();
    });
    row.appendChild(removeBtn);

    return row;
  }

  function render() {
    const items = getItems();
    container.innerHTML = '';
    items.forEach((_, idx) => container.appendChild(buildRow(items, idx)));
  }

  /** Appends `newItem` (a plain value for primitive fields, else an object keyed by field.key). Returns false (no-op) if it fails the primary field's uniqueness check. */
  function addItem(newItem) {
    const value = primaryValue(newItem, fields);
    if (unique && hasValue(value)) return false;
    getItems().push(newItem);
    notify();
    render();
    return true;
  }

  render();
  return { render, addItem };
}
