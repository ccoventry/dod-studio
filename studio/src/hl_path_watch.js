// hl_path_watch.js
// Everything that reads Configuration's hl.exe path: the map-warning banner,
// demo links, the Commands tab's config warnings and the Master Queue's
// packet-entity marks (#207). Kept out of main.js, which only wires (#683).

import { initMapWarnings } from './map_warnings.js';
import { initDemoLinks } from './demo_links.js';
import { refreshInitCommandWarnings } from './capture_pane.js';
import { refreshPacketEntityLimit } from './packet_entity_limit.js';

/** The hl.exe path as typed now. */
export const readHlPath = () => document.querySelector('#hl-path-input')?.value?.trim() || '';

/** @param {object} app  renderQueue() redraws the Master Queue. */
export function startHlPathWatch({ renderQueue }) {
  // Read at click time, not captured: the path can be set after a scan has
  // already run and left the banner up.
  initMapWarnings(readHlPath);
  initDemoLinks(readHlPath);
  // Scanned once the persisted path is in the DOM, and again whenever it
  // changes: a config file the app cannot see is exactly what this warns about.
  const hlPathInput = document.querySelector('#hl-path-input');
  if (!hlPathInput) return;
  refreshInitCommandWarnings();
  hlPathInput.addEventListener('change', () => refreshInitCommandWarnings());
  // Which engine it is decides which demos the Master Queue marks (#207).
  const markDemosOverLimit = () => refreshPacketEntityLimit(hlPathInput.value.trim()).then(renderQueue);
  markDemosOverLimit();
  hlPathInput.addEventListener('change', markDemosOverLimit);
}
