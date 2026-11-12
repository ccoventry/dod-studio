// agr_rate.js
//
// The rate AGR mode records at. AGR's rate is its own, not Capture FPS: an
// AGR take is rebuilt in Blender, and a high recorded rate makes those
// renders far slower, so following a 300 fps Capture FPS was the wrong
// default for it. An empty AGR FPS (saved as 0) records at DEFAULT_AGR_FPS.

/** Mirrors `native::patch::DEFAULT_AGR_FPS`; a test in
 *  `studio/src-tauri/src/capture_manager.rs` checks the two agree. */
export const DEFAULT_AGR_FPS = 30;

/** The rate a batch records AGR at for a saved `agr_fps` (0, absent or
 *  anything not a positive number = `DEFAULT_AGR_FPS`), as
 *  `PatcherConfig::effective_agr_fps` does. */
export function effectiveAgrFps(agrFps) {
  return Number.isFinite(agrFps) && agrFps > 0 ? agrFps : DEFAULT_AGR_FPS;
}
