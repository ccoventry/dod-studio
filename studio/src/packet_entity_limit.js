// packet_entity_limit.js
// Demos over the configured engine's entity limit (#207). Each scanned demo
// carries the most entities any one snapshot holds (`peak_packet_entities`);
// the engine closes to the desktop at the first snapshot over its
// MAX_PACKET_ENTITIES, 256 on the pre-Anniversary build and 1024 on the 25th
// Anniversary one. Studio plays such a demo from a trimmed copy instead
// (native::patch::trimmed_copies), so the Master Queue only marks them, and a
// preview says when it plays one (preview_trim_status.js).

import { enginePacketEntityLimit } from './ipc_bridge.js';
import { STRINGS } from './strings.js';

/** The pre-Anniversary engine's limit, assumed until the engine is read. */
export const PRE_ANNIVERSARY_LIMIT = 256;

let limit = PRE_ANNIVERSARY_LIMIT;
let limitFor = null;

/** The limit of the engine beside `gamePath` (hl.exe), read once per path.
 *  An unreadable or unset path keeps the pre-Anniversary 256. */
export async function refreshPacketEntityLimit(gamePath) {
  if (!gamePath) {
    limit = PRE_ANNIVERSARY_LIMIT;
    limitFor = null;
    return limit;
  }
  if (gamePath === limitFor) return limit;
  const read = await enginePacketEntityLimit(gamePath);
  limit = Number.isInteger(read) && read > 0 ? read : PRE_ANNIVERSARY_LIMIT;
  limitFor = gamePath;
  return limit;
}

/** The limit in effect. */
export function packetEntityLimit() {
  return limit;
}

/** Whether a demo goes over `max` entities in a snapshot. A demo scanned
 *  before the count existed has none, and isn't flagged. */
export function overPacketEntityLimit(demo, max = limit) {
  return Number.isInteger(demo?.peak_packet_entities) && demo.peak_packet_entities > max;
}

/** The Master Queue's mark for a demo over the limit, or null. */
export function overLimitBadge(demo) {
  if (!overPacketEntityLimit(demo)) return null;
  const badge = document.createElement('span');
  badge.className = 'entity-limit-badge';
  badge.textContent = STRINGS.ENTITY_LIMIT.BADGE;
  badge.title = STRINGS.ENTITY_LIMIT.badgeTitle(demo.peak_packet_entities, limit)
    + (limit === PRE_ANNIVERSARY_LIMIT ? ` ${STRINGS.ENTITY_LIMIT.ANNIVERSARY_HINT}` : '');
  return badge;
}
