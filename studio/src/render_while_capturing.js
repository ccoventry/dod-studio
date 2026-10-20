// render_while_capturing.js
//
// "Render while capturing" (#861): each take a capture batch records is
// rendered as soon as it lands, instead of after the whole batch, and with
// "After a clip is finished" set to delete (#644) it goes once its clip is
// done. The batch holds between clips when the next one wouldn't fit until
// those renders have freed the room.
//
// The backend follows the batch (capture_room_manager.rs): it sends
// `capture_clip_ready` once a take stops growing, runs one render at a time
// while the capture runs, decides when the batch holds and reports it with
// `capture_room_status`. This module is the setting, queueing each ready take
// through the Render tab's own queue (the same payload the finish step, #440,
// uses), and the status line while the batch holds.
//
// Everything above the page wiring is DOM-free so it can be unit tested
// (render_while_capturing.test.js).
import { listen } from '@tauri-apps/api/event';
import { queueRenderBatch, startQueuedRender } from './ipc_bridge.js';
import { renderPayloadFor } from './finish_clips.js';
import { STRINGS } from './strings.js';
import { showToast } from './toast.js';

/** The line under the setting, from it and "After a clip is finished". */
export function settingHint(enabled, takeChoice) {
  if (!enabled) return STRINGS.RENDER_WHILE_CAPTURING.HINT_OFF;
  return takeChoice === 'delete'
    ? STRINGS.RENDER_WHILE_CAPTURING.HINT_ON_DELETE
    : STRINGS.RENDER_WHILE_CAPTURING.HINT_ON_KEEP;
}

/** Whether an error from `start_queued_render` only means it already runs. */
export function alreadyRendering(err) {
  return /already in progress/i.test(String(err?.message || err || ''));
}

/** Whether an error from `queue_render_batch` means another scan holds it. */
export function scanBusy(err) {
  return /scan for takes is already running/i.test(String(err?.message || err || ''));
}

/**
 * Queues each ready take's render, one at a time, and starts the Render tab
 * when it isn't running. A scan the user started holds the queue for a
 * moment: the take is tried again.
 *
 * deps:
 *   queue(payload) -> Promise       queue_render_batch
 *   start() -> Promise              start_queued_render
 *   payloadFor(clip) -> { payload } | { skip }
 *   wait(ms) -> Promise             a pause before trying again
 *   onSkip(reason), onError(err)
 */
export function createClipRenderQueue({ queue, start, payloadFor, wait, onSkip, onError, retries = 20 }) {
  let chain = Promise.resolve();

  async function render(clip) {
    const built = payloadFor(clip);
    if (!built.payload) {
      onSkip?.(built.skip);
      return;
    }
    for (let attempt = 0; ; attempt += 1) {
      try {
        await queue(built.payload);
        break;
      } catch (err) {
        if (scanBusy(err) && attempt < retries) {
          await wait(500);
          continue;
        }
        onError?.(err);
        return;
      }
    }
    try {
      await start();
    } catch (err) {
      // The running batch picks the new row up on its own.
      if (!alreadyRendering(err)) onError?.(err);
    }
  }

  return {
    /** `clip` is a `capture_clip_ready` payload. Resolves once it is queued. */
    clipReady(clip) {
      chain = chain.then(() => render(clip));
      return chain;
    },
  };
}

/** The Capture footer's line while the batch holds for space. */
export function roomStatusLine(status, formatSize) {
  if (!status?.state) return null;
  const short = status.short_bytes ? formatSize(status.short_bytes) : null;
  const eta = Number(status.eta_seconds);
  const minutes = Number.isFinite(eta) && eta > 0 ? Math.max(1, Math.round(eta / 60)) : null;
  return status.state === 'held'
    ? STRINGS.RENDER_WHILE_CAPTURING.heldLine(short, minutes)
    : STRINGS.RENDER_WHILE_CAPTURING.holdingLine(short);
}

// ── Page wiring ──────────────────────────────────────────────────────────────

let enabled = false;

function sizeText(bytes) {
  const gb = (Number(bytes) || 0) / (1024 * 1024 * 1024);
  return gb >= 1
    ? STRINGS.RENDER.fileSizeGb(gb.toFixed(1))
    : STRINGS.RENDER.fileSizeMb(((Number(bytes) || 0) / (1024 * 1024)).toFixed(0));
}

function paint() {
  const box = document.querySelector('#config-render-while-capturing');
  if (box) box.checked = enabled;
  const hint = document.querySelector('#config-render-while-capturing-hint');
  const choice = document.querySelector('#config-take-after-finish')?.value;
  if (hint) hint.textContent = settingHint(enabled, choice);
}

/** What settings_sync.js saves. */
export function getRenderWhileCapturing() {
  return { render_while_capturing: enabled };
}

/** What settings_sync.js loads. */
export function setRenderWhileCapturing(value) {
  enabled = !!value;
  paint();
}

/** `onSettingsChange` persists the settings when the user changes it. */
export function initRenderWhileCapturing({ onSettingsChange } = {}) {
  paint();
  document.querySelector('#config-render-while-capturing')?.addEventListener('change', (e) => {
    enabled = e.target.checked;
    paint();
    onSettingsChange?.();
  });
  document.querySelector('#config-take-after-finish')?.addEventListener('change', paint);

  const renders = createClipRenderQueue({
    queue: queueRenderBatch,
    start: startQueuedRender,
    payloadFor: (clip) => renderPayloadFor([clip.take_folder], clip.capture_fps, clip.capture_mode),
    wait: (ms) => new Promise((resolve) => { setTimeout(resolve, ms); }),
    onSkip: (reason) => showToast(
      reason === 'custom_args' ? STRINGS.FINISH.SKIPPED_CUSTOM_ARGS : STRINGS.FINISH.SKIPPED_NO_EXPORT_DIR,
      'warning',
      8000,
    ),
    onError: (err) => showToast(STRINGS.RENDER_WHILE_CAPTURING.queueFailed(err), 'error', 8000),
  });

  // The backend only sends this with the setting on, for the batch it was
  // on for when it started.
  listen('capture_clip_ready', (event) => {
    renders.clipReady(event.payload || {});
  }).catch((err) => console.error('Failed to register capture_clip_ready listener:', err));

  listen('capture_room_status', (event) => {
    const line = roomStatusLine(event.payload, sizeText);
    if (!line) return;
    const statusEl = document.querySelector('#batch-status');
    if (statusEl) statusEl.textContent = line;
  }).catch((err) => console.error('Failed to register capture_room_status listener:', err));

  listen('capture_room_stopped', (event) => {
    showToast(STRINGS.RENDER_WHILE_CAPTURING.stoppedToast(event.payload?.clip, event.payload?.reason), 'warning', 15000);
  }).catch((err) => console.error('Failed to register capture_room_stopped listener:', err));
}
