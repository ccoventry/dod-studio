// context_menu.js
// The app's one right-click menu (#1111): themed like the File/Help dropdowns,
// opened at the pointer or beside a focused row, usable from the keyboard,
// and closed by anything that moves the page out from under it. Each place
// that wants a menu builds its items and calls open(); nothing here knows
// what a demo or a highlight is.

/** Gap kept between the menu and the window edge. */
export const VIEWPORT_MARGIN = 4;

/**
 * Where a menu of `size` goes when asked for `at`, so all of it stays inside
 * `viewport`: slid back from the right and bottom edges, never past the top
 * or left ones (a menu taller than the window starts at the top margin).
 */
export function clampToViewport(at, size, viewport, margin = VIEWPORT_MARGIN) {
  const maxX = viewport.width - size.width - margin;
  const maxY = viewport.height - size.height - margin;
  return {
    x: Math.max(margin, Math.min(at.x, maxX)),
    y: Math.max(margin, Math.min(at.y, maxY)),
  };
}

/** Items as the menu draws them: falsy entries (a `cond && item` slot) dropped,
 *  and separators trimmed so none leads, trails or doubles up. */
export function normalizeItems(items) {
  const out = [];
  for (const item of items.filter(Boolean)) {
    if (item.separator) {
      if (out.length === 0 || out[out.length - 1].separator) continue;
    }
    out.push(item);
  }
  while (out.length && out[out.length - 1].separator) out.pop();
  return out;
}

/**
 * Whether an item can be chosen and what its tooltip says. `disabled` is
 * `true` or a string; a string is the reason, shown on hover so a dimmed item
 * explains itself instead of just not responding.
 */
export function itemState(item) {
  const disabled = !!item.disabled;
  const reason = typeof item.disabled === 'string' ? item.disabled : '';
  return { enabled: !disabled, tooltip: disabled ? reason : (item.title || '') };
}

/**
 * The index the arrow keys land on from `from` (-1 for none yet), stepping
 * `dir` (1 or -1) over separators and dimmed items and wrapping around.
 * -1 when nothing can be chosen.
 */
export function nextIndex(items, from, dir) {
  const n = items.length;
  for (let step = 1; step <= n; step++) {
    const i = ((from + dir * step) % n + n) % n;
    if (!items[i].separator && itemState(items[i]).enabled) return i;
  }
  return -1;
}

/**
 * @param {object} [opts]
 * @param {Document} [opts.doc]
 * @returns {{ open: Function, close: Function, isOpen: Function }}
 */
export function createContextMenu({ doc = document } = {}) {
  const win = doc.defaultView;
  let menuEl = null;
  let rows = [];        // [{ item, el }] for the choosable items, in order
  let activeIdx = -1;
  let opener = null;    // the element that had focus, to give it back
  let onClose = null;
  let stopListening = () => {};

  const isOpen = () => menuEl !== null;

  function setActive(i) {
    activeIdx = i;
    rows.forEach((r, k) => r.el.classList.toggle('active', k === i));
    if (i >= 0) rows[i].el.focus({ preventScroll: true });
  }

  function close({ restoreFocus = true } = {}) {
    if (!menuEl) return;
    stopListening();
    menuEl.remove();
    menuEl = null;
    rows = [];
    activeIdx = -1;
    const back = opener;
    opener = null;
    const done = onClose;
    onClose = null;
    if (restoreFocus && back && back.isConnected) back.focus({ preventScroll: true });
    if (done) done();
  }

  function choose(i) {
    const { item } = rows[i];
    close();
    item.onSelect?.();
  }

  function onKeydown(e) {
    const items = rows.map((r) => r.item);
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const dir = e.key === 'ArrowDown' ? 1 : -1;
      const next = nextIndex(items, activeIdx, dir);
      if (next >= 0) setActive(next);
    } else if (e.key === 'Home' || e.key === 'End') {
      e.preventDefault();
      const next = e.key === 'Home' ? nextIndex(items, -1, 1) : nextIndex(items, items.length, -1);
      if (next >= 0) setActive(next);
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      if (activeIdx >= 0) choose(activeIdx);
    } else if (e.key === 'Tab') {
      e.preventDefault();
      close();
    }
  }

  /**
   * Shows a menu at `at` (`{ x, y }` in window pixels).
   * @param {object} spec
   * @param {{x:number,y:number}} spec.at
   * @param {Array} spec.items  `{ label, onSelect, disabled?, title?, danger? }`
   *   or `{ separator: true }`; see normalizeItems and itemState.
   * @param {string} [spec.label]  accessible name of the menu
   * @param {boolean} [spec.focusFirst]  start on the first choosable item (a keyboard open)
   * @param {Function} [spec.onClose]  called once when it goes away
   */
  function open({ at, items, label = '', focusFirst = false, onClose: closed = null }) {
    close({ restoreFocus: false });
    const list = normalizeItems(items);
    if (list.length === 0) return;
    opener = doc.activeElement;
    onClose = closed;

    menuEl = doc.createElement('div');
    menuEl.className = 'context-menu';
    menuEl.setAttribute('role', 'menu');
    if (label) menuEl.setAttribute('aria-label', label);
    menuEl.tabIndex = -1;
    rows = [];
    for (const item of list) {
      if (item.separator) {
        const sep = doc.createElement('div');
        sep.className = 'context-menu-separator';
        sep.setAttribute('role', 'separator');
        menuEl.appendChild(sep);
        continue;
      }
      const { enabled, tooltip } = itemState(item);
      const el = doc.createElement('div');
      el.className = 'context-menu-item';
      el.setAttribute('role', 'menuitem');
      el.tabIndex = -1;
      el.textContent = item.label;
      if (tooltip) el.title = tooltip;
      if (item.danger) el.classList.add('danger');
      if (!enabled) {
        el.classList.add('dimmed');
        el.setAttribute('aria-disabled', 'true');
      } else {
        const idx = rows.length;
        el.addEventListener('mousemove', () => { if (activeIdx !== idx) setActive(idx); });
        el.addEventListener('click', () => choose(idx));
        rows.push({ item, el });
      }
      menuEl.appendChild(el);
    }
    // rows holds only the choosable items, so a dimmed one is in the DOM but
    // never in the keyboard order.
    menuEl.style.left = '0px';
    menuEl.style.top = '0px';
    doc.body.appendChild(menuEl);
    const box = menuEl.getBoundingClientRect();
    const spot = clampToViewport(at, { width: box.width, height: box.height },
      { width: win.innerWidth, height: win.innerHeight });
    menuEl.style.left = `${spot.x}px`;
    menuEl.style.top = `${spot.y}px`;

    const outside = (e) => { if (menuEl && !menuEl.contains(e.target)) close({ restoreFocus: false }); };
    const away = () => close({ restoreFocus: false });
    // Scrolling the page closes it; scrolling inside the menu itself doesn't.
    const scrolled = (e) => { if (menuEl && !menuEl.contains(e.target)) away(); };
    menuEl.addEventListener('keydown', onKeydown);
    menuEl.addEventListener('contextmenu', (e) => e.preventDefault());
    doc.addEventListener('pointerdown', outside, true);
    doc.addEventListener('contextmenu', outside, true);
    doc.addEventListener('scroll', scrolled, true);
    doc.addEventListener('wheel', scrolled, { capture: true, passive: true });
    win.addEventListener('blur', away);
    win.addEventListener('resize', away);
    stopListening = () => {
      doc.removeEventListener('pointerdown', outside, true);
      doc.removeEventListener('contextmenu', outside, true);
      doc.removeEventListener('scroll', scrolled, true);
      doc.removeEventListener('wheel', scrolled, true);
      win.removeEventListener('blur', away);
      win.removeEventListener('resize', away);
    };

    // Focus goes to the menu either way so Esc and the arrows work; a
    // keyboard open also lands on the first item, a pointer open waits for the pointer.
    menuEl.focus({ preventScroll: true });
    if (focusFirst) setActive(nextIndex(rows.map((r) => r.item), -1, 1));
  }

  return { open, close, isOpen };
}
