// killfeed_layout_warning.js
// The kill feed's even layout can't draw (#1339): when the hook DLL reports
// that `ds_hud_deathmsg layout even` is waiting and the game's own layout
// shows, a warning line under the capture status line says why and which
// step fixes it, and a toast says it once, so a batch doesn't record every
// clip with the wrong kill feed unseen. The line stays after the batch ends,
// for someone back from an overnight batch, and goes at the next batch's
// start or once the hook says the even layout draws.
//
// Fed by `capture_killfeed_layout` (capture_manager.rs: `{ warning }`, the
// text native's messages/killfeed_layout.rs builds, or null once nothing is
// wrong) and `capture_status`, for when a batch starts. The activity log's
// line is written by the backend (capture_engine/markers.rs).

import { STRINGS } from './strings.js';

let started = false;

/** What a report does: `{ show, text, toast }`, `toast` when `warning` is
 *  new for this batch (`said`: the warnings already toasted). */
export function layoutWarning(payload, said) {
  const warning = typeof payload?.warning === 'string' ? payload.warning.trim() : '';
  if (!warning) return { show: false, text: '', toast: false };
  return { show: true, text: warning, toast: !said.has(warning) };
}

/** Listens for the reports and keeps `el` (the line) up to date. */
export function startKillfeedLayoutWarning({ el, listen, toast = () => {} }) {
  // The capture pane's setup can run again; one set of listeners is enough.
  if (started) return;
  started = true;
  let batchRunning = false;
  const said = new Set();

  const hide = () => {
    if (!el) return;
    el.hidden = true;
    el.textContent = '';
  };

  listen('capture_status', (event) => {
    const running = !!event.payload?.running;
    if (running && !batchRunning) {
      said.clear();
      hide();
    }
    batchRunning = running;
  }).catch((err) => console.error('Failed to register the kill-feed layout status listener:', err));

  listen('capture_killfeed_layout', (event) => {
    const line = layoutWarning(event.payload, said);
    if (!line.show) {
      hide();
      return;
    }
    if (line.toast) {
      said.add(line.text);
      toast(line.text);
    }
    if (!el) return;
    el.hidden = false;
    el.textContent = line.text;
    el.title = STRINGS.KILLFEED_LAYOUT.LINE_TITLE;
  }).catch((err) => console.error('Failed to register the kill-feed layout listener:', err));
}
