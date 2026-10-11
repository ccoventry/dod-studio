// Delete Take (#644): the Render tab's button that sends a finished take's
// folder to the Recycle Bin.
//
// Measure first (the backend checks each folder really is a take and sizes
// it), confirm with the folders and the size, then move them. The backend
// refuses a take that is queued or rendering on its own; the button is off
// for one too, so the refusal is never the first thing anyone sees.
//
// Takes a list throughout, so #70's multi-select can hand it several rows.
import { STRINGS } from './strings.js';
import { JOB_STATUS } from './render_jobs.js';

export const DELETE_STEP = { MEASURING: 'measuring', RECYCLING: 'recycling' };

/** Take folders with a job Queued or Rendering. A take with a HUD pair is two
 *  jobs, and either one being busy keeps the take. */
export function busyTakeFolders(jobs) {
  return new Set(
    (jobs || [])
      .filter((j) => j.take_folder && (j.status === JOB_STATUS.QUEUED || j.status === JOB_STATUS.RENDERING))
      .map((j) => j.take_folder),
  );
}

/** The distinct take folders, in order: two rows of one take are one take. */
export function uniqueTakeFolders(folders) {
  return [...new Set((folders || []).filter(Boolean))];
}

export function totalBytes(sizes) {
  return (sizes || []).reduce((sum, s) => sum + (Number(s.size_bytes) || 0), 0);
}

/** The confirm dialog's content for measured takes. */
export function confirmContent(sizes, formatSize) {
  const n = sizes.length;
  const total = formatSize(totalBytes(sizes));
  return {
    title: STRINGS.RENDER.deleteTakeConfirmTitle(n),
    message: STRINGS.RENDER.deleteTakeConfirmMessage(n, total),
    details: sizes.map((s) => ({
      primary: s.take_key || s.take_folder,
      secondary: `${formatSize(s.size_bytes)} · ${s.take_folder}`,
      title: s.take_folder,
    })),
    footer: STRINGS.RENDER.DELETE_TAKE_CONFIRM_FOOTER,
    confirmLabel: STRINGS.RENDER.DELETE_TAKE_CONFIRM_BUTTON,
  };
}

/** The toast for a finished delete: `{ message, level }`. */
export function resultToast(result, formatSize) {
  const deleted = result?.deleted || [];
  const failed = result?.failed || [];
  if (failed.length === 0) {
    return { message: STRINGS.RENDER.takesDeleted(deleted.length, formatSize(totalBytes(deleted))), level: 'success' };
  }
  return {
    message: STRINGS.RENDER.takesDeleteFailed(deleted.length, failed.length, failed[0].error),
    level: 'error',
  };
}

/** The line under the progress bar. */
export function progressLine(step, done, total) {
  return step === DELETE_STEP.RECYCLING
    ? STRINGS.RENDER.deleteTakeRecycling(done, total)
    : STRINGS.RENDER.deleteTakeMeasuring(done, total);
}

/**
 * The click flow. `measure`/`remove` are the IPC calls, `confirm` the themed
 * confirm, `showProgress({ step, done, total } | null)` paints the bar, and
 * `onBusyChange(busy)` lets the table turn every Delete Take off meanwhile.
 * `listen` subscribes to the backend's `take_delete_progress` events.
 */
export function createTakeDelete({ measure, remove, confirm, showToast, formatSize, showProgress, onBusyChange, listen }) {
  let running = false;

  listen?.('take_delete_progress', (event) => {
    const { step, done, total } = event?.payload || {};
    if (running && step) showProgress({ step, done, total });
  });

  function setRunning(value) {
    running = value;
    onBusyChange?.(value);
  }

  async function deleteTakes(folders) {
    const takeFolders = uniqueTakeFolders(folders);
    if (running || takeFolders.length === 0) return;
    setRunning(true);
    try {
      showProgress({ step: DELETE_STEP.MEASURING, done: 0, total: takeFolders.length });
      let sizes;
      try {
        sizes = await measure(takeFolders);
      } catch (err) {
        showToast(STRINGS.RENDER.deleteTakeRefused(err), 'error', 8000);
        return;
      }
      showProgress(null);
      const content = confirmContent(sizes, formatSize);
      const ok = await confirm(content.message, content);
      if (!ok) return;

      showProgress({ step: DELETE_STEP.RECYCLING, done: 0, total: takeFolders.length });
      let result;
      try {
        result = await remove(takeFolders);
      } catch (err) {
        showToast(STRINGS.RENDER.deleteTakeRefused(err), 'error', 8000);
        return;
      }
      const toast = resultToast(result, formatSize);
      showToast(toast.message, toast.level, toast.level === 'error' ? 10000 : undefined);
    } finally {
      showProgress(null);
      setRunning(false);
    }
  }

  return { deleteTakes, isRunning: () => running };
}
