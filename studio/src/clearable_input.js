// clearable_input.js
// An × at the right-hand end of a text box that clears it (#529). Shown only
// while the box has text. Clearing fires the box's own `input` event, so
// whatever filters on typing re-runs exactly as if the text had been deleted,
// and puts focus back in the box. Esc in the box does the same.

/**
 * Adds the × to `input`. `label` is its tooltip and accessible name. Safe to
 * call twice on one box.
 */
export function makeClearable(input, label) {
  if (!input || input.dataset.clearable) return;
  input.dataset.clearable = '1';

  const wrap = document.createElement('span');
  wrap.className = 'clearable-input';
  input.parentNode.insertBefore(wrap, input);
  wrap.appendChild(input);
  // Room for the ×, so long text doesn't run under it.
  input.style.paddingRight = '20px';

  const x = document.createElement('button');
  x.type = 'button';
  x.className = 'clearable-x';
  x.textContent = '×';
  x.title = label;
  x.setAttribute('aria-label', label);
  wrap.appendChild(x);

  const sync = () => { x.hidden = input.value === ''; };
  const clear = () => {
    input.value = '';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.focus();
  };

  input.addEventListener('input', sync);
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && input.value !== '') {
      e.preventDefault();
      e.stopPropagation();
      clear();
    }
  });
  // mousedown would take focus from the box first; keep it there.
  x.addEventListener('mousedown', (e) => e.preventDefault());
  x.addEventListener('click', clear);
  sync();
}
