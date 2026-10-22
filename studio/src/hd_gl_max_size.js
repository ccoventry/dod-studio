// hd_gl_max_size.js
// The HD page's gl_max_size line (#680): what the selected install runs
// with, and a warning when that shows HD files smaller than the size they
// are built at. The value comes from native::hd::gl_max_size (the install's
// configs, then Initial Commands); nothing here writes the game's configs.

import { STRINGS } from './strings.js';

/**
 * @param {{value: string, shows_at: number, source: {kind: string, file?: string, line?: number}} | null | undefined} gl
 * @param {number} cap  the build's largest side
 * @returns {{text: string, low: boolean} | null}  null when there's no report
 */
export function glMaxSizeLine(gl, cap) {
  if (!gl) return null;
  const source = STRINGS.HD.glMaxSizeSource(gl.source);
  if (gl.shows_at >= cap) {
    return { text: STRINGS.HD.glMaxSizeOk(gl.value, source, gl.shows_at), low: false };
  }
  const fix = gl.source?.kind === 'initial_commands'
    ? STRINGS.HD.GL_MAX_SIZE_FIX_INITIAL
    : STRINGS.HD.GL_MAX_SIZE_FIX_CFG;
  return { text: STRINGS.HD.glMaxSizeLow(gl.value, source, gl.shows_at, cap, fix), low: true };
}
