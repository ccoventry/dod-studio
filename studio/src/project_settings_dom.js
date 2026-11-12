// project_settings_dom.js
// The pieces every tab of the Project settings page (#1085) is drawn from:
// elements, rows, cards, chips and number boxes, so the tabs look alike and
// share their markup (the `ps-` classes in project_settings_page.css).

/** An element with `props` (class, text, title, and any other property)
 *  and `children`. */
export function el(tag, props = {}, children = []) {
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
export function row(label, hint, controls, { id } = {}) {
  return el('div', { class: 'ps-row', dataset: id ? { row: id } : {} }, [
    el('div', { class: 'ps-row-label' }, [
      el('div', { class: 'ps-row-title', text: label }),
      hint ? el('div', { class: 'ps-row-hint', text: hint }) : null,
    ]),
    el('div', { class: 'ps-row-controls' }, controls),
  ]);
}

/** A card for one section. */
export function card(id, title, sub, rows) {
  return el('section', { class: 'ps-card', id: `ps-section-${id}` }, [
    el('h3', { class: 'ps-card-title' }, [
      document.createTextNode(title),
      sub ? el('span', { class: 'ps-card-sub', text: sub }) : null,
    ]),
    ...rows,
  ]);
}

/** A row of chips, one picked. `choices` are `{ value, label, title }`. */
export function chips(name, choices, picked, onPick) {
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
export function numberBox(name, value, onValue, { step = '1', width = '84px', title } = {}) {
  const input = el('input', {
    type: 'number', class: 'ps-number', step, value: value === undefined || value === null ? '' : String(value),
    title: title || '', dataset: { field: name },
  });
  input.style.width = width;
  input.addEventListener('input', () => onValue(input.value.trim() === '' ? NaN : Number(input.value)));
  return input;
}

export const unit = (text) => el('span', { class: 'ps-unit', text });

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
