// capture_precheck.js — what the Capture footer shows while Start Capture
// Batch checks everything before the batch starts (#953): OBS, disk space,
// the picked demos, the maps, the paths, a running game and Steam. Start is
// greyed out and the bar slides from the click, so the click never looks
// lost; a check that stops the batch brings Start back and says why.
// capture_pane.js calls it at each step.
//
// The sliding bar is the page's own `.progress-bar-container.indeterminate`;
// it can move to progress_view.js once #951 lands.
import { STRINGS } from './strings.js';
import { setStatusLine } from './status_line.js';

/// The line for `step`; `done`/`total` count the picked demos read so far.
export function precheckLine(step, done = 0, total = 0) {
  const S = STRINGS.CAPTURE_PRECHECK;
  switch (step) {
    case 'obs': return S.OBS;
    case 'disk': return S.DISK;
    case 'demos': return total > 0 ? S.demosCounted(Math.min(done, total), total) : S.DEMOS;
    case 'maps': return S.MAPS;
    case 'paths': return S.PATHS;
    case 'game': return S.GAME;
    case 'steam': return S.STEAM;
    default: return S.STARTING;
  }
}

/**
 * The footer while the checks run. `refreshStart()` puts Start back the way
 * the disk-space guard wants it (capture_pane.js' refreshLaunchGuard).
 */
export function createCapturePrecheck({ startBtn, statusEl, progressContainer, refreshStart }) {
  let checking = false;
  let shownBefore = '';

  function slide(on) {
    if (!progressContainer) return;
    progressContainer.classList.toggle('indeterminate', on);
  }

  return {
    isChecking: () => checking,

    /// The click: Start greys out and the first line shows at once.
    begin() {
      checking = true;
      if (startBtn) startBtn.disabled = true;
      if (progressContainer) {
        shownBefore = progressContainer.style.display;
        progressContainer.style.display = 'block';
      }
      slide(true);
      setStatusLine(statusEl, precheckLine('start'));
    },

    step(name, done, total) {
      if (checking) setStatusLine(statusEl, precheckLine(name, done, total));
    },

    /// A check stopped the batch: `why` is what its toast said (or nothing,
    /// when the user said no to a question, or the check set its own line).
    stop(why = STRINGS.CAPTURE_PRECHECK.NOT_STARTED) {
      if (!checking) return;
      checking = false;
      slide(false);
      if (progressContainer) progressContainer.style.display = shownBefore;
      if (why) setStatusLine(statusEl, why);
      refreshStart?.();
    },

    /// Every check passed: the batch's own lines take over from here.
    handOver() {
      checking = false;
      slide(false);
    },
  };
}
