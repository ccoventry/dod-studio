// demo_drop.js
// Dragging demos and folders from Windows onto the Highlights page (#530).
// Tauri delivers a native drop as a webview event with full paths and the
// pointer position (the browser's own HTML5 drop has no real paths), so this
// works out which of the two drop zones, Directory Scan & Management or the
// Master Demo Queue, is under the pointer. Both zones are highlighted while
// something is dragged over the window, the one under the pointer more; a
// drop anywhere else is ignored.

import { getCurrentWebview } from '@tauri-apps/api/webview';
import { sortDroppedPaths } from './ipc_bridge.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

const ZONE_SELECTORS = ['#scan-panel', '#master-pane'];

/**
 * The zone `point` (CSS pixels, from the webview's top-left) is over, or
 * null. A zone that isn't shown (another page is open) never matches.
 * `zones`: [{ el, rect: { left, top, right, bottom }, shown }].
 */
export function zoneAt(zones, point) {
  if (!point) return null;
  const hit = zones.find(({ rect, shown }) => shown
    && point.x >= rect.left && point.x < rect.right
    && point.y >= rect.top && point.y < rect.bottom);
  return hit ? hit.el : null;
}

/** The event's physical-pixel position in CSS pixels. */
export function cssPoint(position, scale = 1) {
  if (!position) return null;
  const s = scale > 0 ? scale : 1;
  return { x: position.x / s, y: position.y / s };
}

function currentZones() {
  return ZONE_SELECTORS
    .map((sel) => document.querySelector(sel))
    .filter(Boolean)
    .map((el) => {
      const r = el.getBoundingClientRect();
      return { el, rect: r, shown: el.offsetParent !== null && r.width > 0 && r.height > 0 };
    });
}

/**
 * @param {object} app  addDroppedPaths(files, folders) and isScanning() from
 *   queue_scan.js. `subscribe(handler)` registers for the drag events; the
 *   e2e harness passes its own, the app the webview's.
 */
export function createDemoDrop({
  addDroppedPaths,
  isScanning = () => false,
  subscribe = (handler) => getCurrentWebview().onDragDropEvent(handler),
}) {
  function highlight(active, hovered = null) {
    currentZones().forEach(({ el, shown }) => {
      el.classList.toggle('drop-zone-active', active && shown);
      el.classList.toggle('drop-zone-hover', active && el === hovered);
    });
  }

  async function onDrop(paths, point) {
    highlight(false);
    if (!zoneAt(currentZones(), point) || !paths || paths.length === 0) return;
    if (isScanning()) {
      showToast(STRINGS.DROP.BUSY_TOAST, 'warning', 6000);
      return;
    }
    let sorted;
    try {
      sorted = await sortDroppedPaths(paths);
    } catch {
      return; // already toasted in ipc_bridge
    }
    const { demos = [], folders = [], skipped = [] } = sorted || {};
    if (skipped.length > 0) showToast(STRINGS.DROP.skippedToast(skipped), 'warning', 10000);
    if (demos.length + folders.length > 0) await addDroppedPaths(demos, folders);
  }

  async function handle(event) {
    const p = event?.payload || {};
    const point = cssPoint(p.position, window.devicePixelRatio || 1);
    if (p.type === 'enter' || p.type === 'over') {
      highlight(true, zoneAt(currentZones(), point));
    } else if (p.type === 'drop') {
      await onDrop(p.paths, point);
    } else {
      highlight(false);
    }
  }

  Promise.resolve()
    .then(() => subscribe(handle))
    .catch((err) => console.error('Error listening for dropped files:', err));

  return { handle };
}
