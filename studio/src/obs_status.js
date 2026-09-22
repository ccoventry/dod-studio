// obs_status.js
// Tracks whether the most recent obs_test_connection check succeeded, so
// refreshLaunchGuard (capture_pane.js) can gate Start Capture Batch on it
// (issue #147: "nothing today stops a batch from starting in OBS mode with
// OBS closed or unauthenticated") without re-querying OBS itself on every
// guard refresh. main.js already runs the real check — manually via Test
// Connection, automatically on switching into OBS mode, and automatically
// at startup when OBS mode is the persisted choice — and calls
// refreshLaunchGuard() right after, so the gate reflects it promptly.
//
// A tiny standalone module rather than living in main.js or capture_pane.js
// so both can import it without a circular dependency between them.

let connected = null; // null = never checked yet

export function setObsConnected(value) {
  connected = value === true;
}

export function isObsConnected() {
  return connected === true;
}

/** False only before the very first check has resolved either way. */
export function obsConnectionChecked() {
  return connected !== null;
}
