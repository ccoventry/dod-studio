// split_resize.js
// Drag-to-resize for a split handle: one pane before the handle grows or
// shrinks as it moves. Shared by the Explorer sidebar, Capture Studio's two
// panes and the Demo Analyzer's Demos panel (#47). What a size means, where it
// is stored and what counts as the default stay with the caller.

/** Keeps `size` inside [min, max]; when the room is too small for min, min wins. */
export function clampSize(size, min, max) {
  return Math.min(Math.max(max, min), Math.max(min, size));
}

/** The stored permille of `total` that `px` is, rounded to a whole number. */
export function pxToPermille(px, total) {
  return total > 0 ? Math.round((px / total) * 1000) : 0;
}

/**
 * Makes `handle` drag-resizable along one axis.
 * @param {object} options
 * @param {HTMLElement} options.handle  The bar the user grabs.
 * @param {'x'|'y'} options.axis  x = pane to the left of the handle, y = pane above it.
 * @param {() => number} options.measure  The pane's current size in px.
 * @param {() => {min: number, max: number}} options.limits  Read on every move, so a window resize is honoured.
 * @param {(px: number) => void} options.apply  Sets the pane's size while dragging.
 * @param {(px: number) => void} options.commit  Called once on release with the final size.
 * @param {() => void} options.reset  Called on a double-click: back to the default.
 */
export function attachSplitResize({ handle, axis, measure, limits, apply, commit, reset }) {
  const client = axis === 'x' ? 'clientX' : 'clientY';
  let startPointer = 0;
  let startSize = 0;
  let lastSize = 0;
  let moved = false;

  function onMove(e) {
    // The button came up where no mouseup reached us (window blur, alt-tab).
    if (e.buttons === 0) { onUp(); return; }
    const { min, max } = limits();
    moved = true;
    lastSize = clampSize(startSize + (e[client] - startPointer), min, max);
    apply(lastSize);
  }

  function onUp() {
    handle.classList.remove('resizing');
    document.removeEventListener('mousemove', onMove);
    document.removeEventListener('mouseup', onUp);
    // A plain click (the first half of a double-click) keeps the layout as it was.
    if (moved) commit(lastSize);
  }

  handle.addEventListener('mousedown', (e) => {
    e.preventDefault();
    startPointer = e[client];
    startSize = measure();
    lastSize = startSize;
    moved = false;
    handle.classList.add('resizing');
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  });
  handle.addEventListener('dblclick', reset);
}
