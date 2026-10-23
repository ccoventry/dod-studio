// searchable_select.js
// A dropdown you can type into: a text box styled like a <select>. Clicking
// it opens the list of every option; typing filters the list; a click or
// Enter picks an option, closes the list and shows the picked option's text
// in place of what was typed. Esc, or leaving the box without a pick, puts
// the picked option's text back. Arrow keys move through the list.
//
// The pure parts (what a filter leaves, which row the arrows land on) are
// exported for tests; the e2e harness drives the rest
// (tests/e2e/searchable-select.html).

/** The options whose text contains `typed`, any case; all of them for none. */
export function matching(options, typed) {
  const want = String(typed || '').trim().toLowerCase();
  if (!want) return options;
  return options.filter((o) => o.text.toLowerCase().includes(want));
}

/** The highlighted row after moving `step` from `index` among `count` rows,
 *  stopping at either end; -1 when there are none. */
export function moveIndex(index, step, count) {
  if (count <= 0) return -1;
  if (index < 0) return step > 0 ? 0 : count - 1;
  return Math.min(count - 1, Math.max(0, index + step));
}

/**
 * Turns `input` into a searchable dropdown. `onPick(value)` runs when an
 * option is picked that isn't the current one; `noMatch` is the list's line
 * when the filter leaves nothing. Returns `{ setOptions, setValue }`: the
 * options as `{ value, text, title }` rows, and the value to show as picked.
 * A row may also carry `action: { label, title, run }`, a button at its end,
 * or `command()`, which a pick runs instead of picking the row.
 */
export function createSearchableSelect(input, { onPick, noMatch = 'No match' }) {
  let options = [];
  let value = '';
  let shown = [];
  let active = -1;
  let typed = false;
  // A click that focuses the box would put the caret down on mouseup and
  // undo the select-all the focus did.
  let justFocused = false;

  input.classList.add('search-select-input');
  input.autocomplete = 'off';
  input.setAttribute('role', 'combobox');
  input.setAttribute('aria-expanded', 'false');
  const wrap = document.createElement('span');
  wrap.className = 'search-select';
  input.parentNode.insertBefore(wrap, input);
  wrap.appendChild(input);
  const panel = document.createElement('div');
  panel.className = 'search-select-panel';
  panel.setAttribute('role', 'listbox');
  panel.hidden = true;
  wrap.appendChild(panel);

  const pickedText = () => options.find((o) => o.value === value)?.text ?? '';

  function render() {
    shown = typed ? matching(options, input.value) : options;
    panel.replaceChildren(...shown.map((o, i) => {
      const row = document.createElement('div');
      row.className = 'search-select-option';
      row.setAttribute('role', 'option');
      row.textContent = o.text;
      row.title = o.title || '';
      row.classList.toggle('command', !!o.command);
      // A small button at the row's end (install_picker.js's Hide): it runs
      // `o.action.run()` and leaves the list open.
      if (o.action) {
        const btn = document.createElement('button');
        btn.type = 'button';
        btn.className = 'search-select-action';
        btn.textContent = o.action.label;
        btn.title = o.action.title || '';
        btn.addEventListener('mousedown', (e) => {
          e.preventDefault();
          e.stopPropagation();
          o.action.run();
        });
        row.appendChild(btn);
      }
      row.classList.toggle('picked', o.value === value);
      row.classList.toggle('active', i === active);
      row.addEventListener('mouseenter', () => {
        active = i;
        for (const [j, el] of [...panel.children].entries()) el.classList.toggle('active', j === i);
      });
      // mousedown, so the box keeps focus and doesn't close the list first.
      row.addEventListener('mousedown', (e) => {
        e.preventDefault();
        pick(o);
      });
      return row;
    }));
    if (!shown.length) {
      const empty = document.createElement('div');
      empty.className = 'search-select-empty';
      empty.textContent = noMatch;
      panel.appendChild(empty);
    }
    panel.children[active]?.scrollIntoView({ block: 'nearest' });
  }

  function open() {
    if (!panel.hidden) return;
    typed = false;
    active = options.findIndex((o) => o.value === value);
    panel.hidden = false;
    input.setAttribute('aria-expanded', 'true');
    render();
  }

  function close() {
    panel.hidden = true;
    input.setAttribute('aria-expanded', 'false');
    typed = false;
    input.value = pickedText();
  }

  function pick(option) {
    // A row that does something instead of being picked ("Show hidden
    // installs"): the list stays open for the caller's setOptions.
    if (option.command) {
      option.command();
      return;
    }
    const changed = option.value !== value;
    value = option.value;
    close();
    if (changed) onPick(option.value);
  }

  input.addEventListener('mousedown', () => {
    if (document.activeElement === input) {
      if (panel.hidden) {
        open();
        input.select();
        justFocused = true;
      } else {
        close();
      }
    }
  });
  input.addEventListener('mouseup', (e) => {
    if (justFocused) e.preventDefault();
    justFocused = false;
  });
  input.addEventListener('focus', () => {
    open();
    // Selected, so the first key typed replaces the picked text.
    input.select();
    justFocused = true;
  });
  input.addEventListener('blur', close);
  input.addEventListener('input', () => {
    if (panel.hidden) open();
    typed = true;
    active = matching(options, input.value).length ? 0 : -1;
    render();
  });
  input.addEventListener('keydown', (e) => {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (panel.hidden) open();
      active = moveIndex(active, e.key === 'ArrowDown' ? 1 : -1, shown.length);
      render();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (!panel.hidden && shown[active]) pick(shown[active]);
    } else if (e.key === 'Escape') {
      if (!panel.hidden) {
        e.preventDefault();
        e.stopPropagation();
        close();
        input.select();
      }
    }
  });

  return {
    setOptions(rows) {
      options = rows;
      if (panel.hidden) input.value = pickedText();
      else render();
    },
    setValue(v) {
      value = v;
      if (panel.hidden) input.value = pickedText();
      else render();
    },
  };
}
