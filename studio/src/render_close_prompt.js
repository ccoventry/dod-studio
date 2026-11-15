// render_close_prompt.js
// Asks before DoD Studio closes while renders are running (#1439). Unlike a
// capture batch, a render cannot go on without Studio: closing ends its
// FFmpeg and removes the clip it was writing (native's
// `stop_renders_for_exit`), so the question says that and its yes is red.

import { themedConfirm } from './themed_confirm.js';
import { getRenderJobs, logFrontendEvent } from './ipc_bridge.js';
import { STRINGS } from './strings.js';

/** How many of the render rows `jobs` (get_render_jobs) are rendering now. */
export function countRendering(jobs) {
  return (jobs ?? []).filter((job) => job.status === 'Rendering').length;
}

/**
 * Whether the window may close: true at once when nothing is rendering (or
 * the job list can't be read, which must never trap the user in the app);
 * otherwise asks, and only "Stop renders and close" says yes.
 */
export async function confirmCloseDuringRenders({ fetchJobs = getRenderJobs, log = logFrontendEvent } = {}) {
  const running = countRendering(await fetchJobs());
  if (running === 0) return true;
  const s = STRINGS.RENDER_CLOSE_MODAL;
  const close = await themedConfirm(s.message(running), {
    title: s.TITLE,
    confirmLabel: s.CLOSE_BUTTON,
    cancelLabel: s.KEEP_OPEN_BUTTON,
    destructive: true,
  });
  log(s.logLine(running, close));
  return close;
}
