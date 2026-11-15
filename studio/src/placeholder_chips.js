// placeholder_chips.js
// The placeholder chips under a name template (#751): one row per group of
// placeholder_catalogue.js, then a Format row for the modifiers and the
// |fallback chip. The Clip Name Template and Rename Demos both draw theirs
// with this, so they look and behave the same.

import { MODIFIERS, WITH_FALLBACK } from './placeholder_catalogue.js';
import { STRINGS } from './strings.js';

/**
 * Fills `box` with the chips.
 * - `groups`: `[{ key, names }]`, from `placeholderGroups()`.
 * - `titleFor(name)`: a usable placeholder's tooltip.
 * - `usable`: the placeholders that can be clicked (default: all); the
 *   rest are greyed out with `unusableTitle(name)` as their tooltip.
 * - `fallbackNames`: the placeholders the |fallback tooltip lists.
 * - `chipClass`: an extra class on each chip, for the page's own styling.
 * - `onInsert(text)`: what a click puts in the template.
 */
export function renderPlaceholderChips(box, {
  groups, titleFor, usable = null, unusableTitle = () => '', fallbackNames, chipClass = '', onInsert,
}) {
  const P = STRINGS.PLACEHOLDERS;
  const chip = (text, title, enabled = true) => {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = `placeholder-chip ${chipClass}`.trim();
    b.textContent = text;
    b.title = title;
    b.disabled = !enabled;
    // Keeps focus (and the cursor) in the field: a blur would fire its
    // change event and rebuild these chips under the click.
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => onInsert(text));
    return b;
  };
  const row = (key, chips) => {
    const label = document.createElement('span');
    label.className = 'placeholder-chip-group-label';
    label.textContent = P.GROUPS[key];
    const list = document.createElement('div');
    list.className = 'placeholder-chip-group';
    list.dataset.group = key;
    list.append(...chips);
    return [label, list];
  };
  // The | chip reads "|fallback" but inserts a bare |, for the word to follow.
  const fallback = chip('|', P.fallbackTitle(fallbackNames.filter((n) => WITH_FALLBACK.has(n)).map((n) => `{${n}}`).join(' ')));
  fallback.textContent = '|fallback';
  box.replaceChildren(
    ...groups.flatMap((g) => row(g.key, g.names.map((name) => (!usable || usable.has(name)
      ? chip(`{${name}}`, titleFor(name))
      : chip(`{${name}}`, unusableTitle(name), false))))),
    ...row('format', [...MODIFIERS.map((m) => chip(`:${m}`, P.DESCRIPTIONS[m])), fallback]),
  );
}
