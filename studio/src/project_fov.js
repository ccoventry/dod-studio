// project_fov.js
// The project's own field of view (#1085, decisions 3 and 4): a project
// always has one, on Project Settings › Picture. One made before it had the
// setting takes, the first time it opens with the New UI on, the value its
// own lines give today: a `mirv_fov <n>` in its Initial Commands, or one in
// a cfg those lines `exec` (read by the backend's read-only scan, native
// picture_fov.rs). With none, the game's 90. A batch then sends it after
// every line, so its look doesn't change. The start-up configs (config.cfg,
// a movie.cfg it execs) run before the batch's lines and are not "its own
// lines": the cfg warnings say the project's value overrides them.

import { projectFovSources, logFrontendEvent } from './ipc_bridge.js';
import { DEFAULT_FOV, fovFromLines, isUsableFov } from './clip_settings_logic.js';
import { blockFov, blockWithFov } from './project_settings_logic.js';
import { projectFovFilledLine } from './project_log.js';

/**
 * The field of view a project with none takes, and where from, as
 * `{ fov, file, line, isDefault }`: `sources` the backend's answer
 * (`{ fromLines }`, null when the scan failed: then the lines alone decide),
 * `lines` its Initial Commands.
 */
export function fovToFill(sources, lines) {
  const stated = sources?.fromLines;
  if (stated && isUsableFov(stated.value)) {
    return { fov: stated.value, file: stated.file ?? null, line: stated.line ?? 0, isDefault: false };
  }
  const typed = sources ? null : fovFromLines(lines);
  if (typed !== null) return { fov: typed, file: null, line: 0, isDefault: false };
  return { fov: DEFAULT_FOV, file: null, line: 0, isDefault: true };
}

/**
 * `block` with a field of view, logged: the same block when it has one,
 * else one with what its `lines` give in the game at `gamePath` (an
 * hl.exe; empty reads the lines alone). `path` names the project in the
 * log. Never writes a config.
 */
export async function withFilledFov(block, { gamePath, lines, path }) {
  if (!block || blockFov(block) !== null) return block;
  const sources = await projectFovSources(gamePath || '', lines || []);
  const fill = fovToFill(sources, lines);
  logFrontendEvent(projectFovFilledLine(path, fill));
  return blockWithFov(block, fill.fov);
}
