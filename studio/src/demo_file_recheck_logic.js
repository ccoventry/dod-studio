// demo_file_recheck_logic.js
// The rules behind demo_file_recheck.js, with no page or IPC: when the
// window's focus may start a look at the queue's demo files, and what one
// look found.

/** The least time between two looks the window's focus starts, so focus
 *  flapping (alt-tabbing, a picker closing) makes one pass, not one per event. */
export const FOCUS_RECHECK_GAP_MS = 2000;

/** Whether a focus look may start at `nowMs`; `lastMs` is when the last one
 *  started, null for never. */
export function focusRecheckDue(lastMs, nowMs, gapMs = FOCUS_RECHECK_GAP_MS) {
  return lastMs === null || nowMs - lastMs >= gapMs;
}

/**
 * What one look found. `checked` holds each row with the path it had when
 * the look started, `exists` one answer per row (files_exist's: true when
 * the file is there).
 * - `gone`: rows without the missing state whose file isn't there.
 * - `back`: rows with the missing state whose file is there.
 * A row whose path changed while the look ran (Locate, Use found copy) is in
 * neither: the answer is about its old path. A row with no answer (not a
 * boolean) is left as it is.
 */
export function fileChanges(checked, exists) {
  const gone = [];
  const back = [];
  checked.forEach(({ demo, path }, i) => {
    const there = exists[i];
    if (demo.path !== path || typeof there !== 'boolean') return;
    if (!there && !demo.missing) gone.push(demo);
    else if (there && demo.missing) back.push(demo);
  });
  return { gone, back };
}
