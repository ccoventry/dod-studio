// studio/src/render_jobs_restore.js
//
// The Render tab's job list after a page load (#771). The queue lives in the
// Rust process, and the page otherwise only learns of it from the next
// `render_jobs_snapshot`; a webview reload (F5, Vite HMR) keeps the queue
// there, so a page that waited for an event showed an empty table over a
// queued batch. Init asks for the list once instead.

/**
 * @param {object} deps
 * @param {() => Promise<object[]|null>} deps.fetchJobs  get_render_jobs; null on an IPC failure
 * @param {(jobs: object[]) => void} deps.applyJobs     paints a job list, the same as a snapshot does
 */
export function createRenderJobsRestore({ fetchJobs, applyJobs }) {
  let snapshotSeen = false;
  return {
    /** Call on every `render_jobs_snapshot`: a snapshot is newer than any reply still on its way. */
    noteSnapshot() {
      snapshotSeen = true;
    },
    /** Asks the backend for its list and paints it. True when it painted. */
    async restore() {
      const list = await fetchJobs();
      if (snapshotSeen || !Array.isArray(list)) return false;
      applyJobs(list);
      return true;
    },
  };
}
