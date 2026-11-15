// render_batch_status.js
// The Render tab's footer counts and the status line once a batch ends:
// rendered, failed and cancelled rows each counted as what they are, so a
// cancelled batch doesn't read as "65 done, Finished" (#818). Pure.

import { JOB_STATUS } from './render_jobs.js';
import { STRINGS } from './strings.js';

const count = (jobs, status) => jobs.filter((j) => j.status === status).length;

/** "3 queued · 1 rendering · 44 done · 21 cancelled": failed and cancelled
 *  only when there are some. "done" counts finished renders only. */
export function queueSummaryText(jobs) {
  const list = jobs || [];
  const parts = [STRINGS.RENDER.queueSummary(
    count(list, JOB_STATUS.QUEUED),
    count(list, JOB_STATUS.RENDERING),
    count(list, JOB_STATUS.FINISHED),
  )];
  const failed = count(list, JOB_STATUS.ERROR);
  const cancelled = count(list, JOB_STATUS.CANCELLED);
  if (failed > 0) parts.push(STRINGS.RENDER.countFailed(failed));
  if (cancelled > 0) parts.push(STRINGS.RENDER.countCancelled(cancelled));
  return parts.join(' · ');
}

/**
 * The status line after a batch: `{ text, status }`, where `status` is the
 * JOB_STATUS whose colour the line takes (the rows' own colours). All
 * cancelled → Cancelled; any failure → Finished with the counts, in the
 * failure colour; some cancelled → Finished with the counts; else Finished.
 * With `duration` (how long the batch ran, in words, #954) a batch that
 * rendered or failed anything says "Done in 7 min 40 s: 10 rendered".
 */
export function batchEndStatus(finished, failed, cancelled, duration = null) {
  if (cancelled > 0 && finished === 0 && failed === 0) {
    return { text: STRINGS.RENDER.STATUS_CANCELLED, status: JOB_STATUS.CANCELLED };
  }
  if (failed === 0 && cancelled === 0 && (duration == null || finished === 0)) {
    return { text: STRINGS.RENDER.STATUS_FINISHED, status: JOB_STATUS.FINISHED };
  }
  const parts = [];
  if (finished > 0) parts.push(STRINGS.RENDER.countRendered(finished));
  if (failed > 0) parts.push(STRINGS.RENDER.countFailed(failed));
  if (cancelled > 0) parts.push(STRINGS.RENDER.countCancelled(cancelled));
  let status = JOB_STATUS.FINISHED;
  if (failed > 0) status = JOB_STATUS.ERROR;
  else if (cancelled > 0) status = JOB_STATUS.CANCELLED;
  return {
    text: duration == null
      ? STRINGS.RENDER.statusFinishedWith(parts.join(', '))
      : STRINGS.RENDER.doneIn(duration, parts.join(', ')),
    status,
  };
}
