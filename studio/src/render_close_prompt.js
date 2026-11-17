// render_close_prompt.js
// Asks before DoD Studio closes while renders or a combine are running
// (#1439, #1496). Unlike a capture batch, neither can go on without Studio:
// closing ends its FFmpeg and removes the file it was writing (native's
// `stop_renders_for_exit`), so the question says that and its yes is red.

import { themedConfirm } from './themed_confirm.js';
import { getRenderJobs, logFrontendEvent } from './ipc_bridge.js';
import { STRINGS } from './strings.js';

/** How many of the render rows `jobs` (get_render_jobs) are rendering now. */
export function countRendering(jobs) {
  return (jobs ?? []).filter((job) => job.status === 'Rendering').length;
}

/**
 * Whether the window may close: true at once when nothing is rendering or
 * combining (or the job list can't be read, which must never trap the user
 * in the app); otherwise asks, and only the red "Stop ... and close" says
 * yes. `isCombining()` says whether a combine is joining now.
 */
export async function confirmCloseDuringRenders({
  fetchJobs = getRenderJobs,
  log = logFrontendEvent,
  isCombining = () => false,
} = {}) {
  const running = countRendering(await fetchJobs());
  const combining = isCombining();
  if (running === 0 && !combining) return true;
  const s = STRINGS.RENDER_CLOSE_MODAL;
  const close = await themedConfirm(s.message(running, combining), {
    title: s.title(running, combining),
    confirmLabel: s.closeButton(running, combining),
    cancelLabel: s.KEEP_OPEN_BUTTON,
    destructive: true,
  });
  log(s.logLine(running, close, combining));
  return close;
}
