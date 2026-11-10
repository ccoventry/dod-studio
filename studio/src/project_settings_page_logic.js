// project_settings_page_logic.js
// The rules behind the Project settings page (#1085, build plan step 6),
// with no DOM, so they are tested on their own: which values the Project tab
// edits, the choices its chips offer, what a pending edit changes, what is
// wrong with it, and the block that Apply writes. project_settings_page.js
// draws the page and holds the pending edits.

import { resolveSettings, followFormEdits, madeWithOf } from './project_settings_logic.js';
import { samePath } from './path_display.js';
import { killGapsFrom, MAX_KILL_GAP_SPEED } from './kill_gaps_setting.js';
import { parseTemplate, DEFAULT_TEMPLATE } from './clip_name.js';
import { sameDriveFolders } from './setup_checklist.js';
import { STRINGS } from './strings.js';

/** Every setting the Project tab edits, by its settings-file name. The
 *  project's block holds each one (project_settings_logic.js's fields). */
export const PROJECT_TAB_KEYS = Object.freeze([
  'capture_mode', 'resolution_width', 'resolution_height',
  'capture_fps', 'obs_capture_fps', 'agr_fps',
  'finish_clips_after_batch', 'finish_codec_obs', 'finish_codec_video', 'finish_codec_frames',
  'record_start_lead', 'record_stop_trail', 'kill_gaps',
  'clip_name_template',
  'target_drives', 'render_export_dirs',
]);

/** Capture with's chips, in the page's order. */
export const CAPTURE_MODES = Object.freeze(['frame_sequence', 'direct_to_video', 'obs', 'agr']);

/** Size's chips other than Custom. */
export const SIZE_CHOICES = Object.freeze([
  { id: '1080p', width: 1920, height: 1080 },
  { id: '1440p', width: 2560, height: 1440 },
  { id: '4k', width: 3840, height: 2160 },
]);

/** Smoothness's chips other than Custom, in frames a second. */
export const RATE_CHOICES = Object.freeze([60, 120, 300]);

/** The Size chip `width` x `height` matches, or 'custom'. */
export function sizeChoiceOf(width, height) {
  return SIZE_CHOICES.find((c) => c.width === width && c.height === height)?.id || 'custom';
}

/** The Smoothness chip `fps` matches, or 'custom'. */
export function rateChoiceOf(fps) {
  return RATE_CHOICES.includes(fps) ? fps : 'custom';
}

/** The frame rate Smoothness edits in capture mode `mode`: OBS keeps its own
 *  live rate, AGR its own positions a second, the HLAE modes Capture FPS. */
export function rateKeyFor(mode) {
  if (mode === 'obs') return 'obs_capture_fps';
  if (mode === 'agr') return 'agr_fps';
  return 'capture_fps';
}

/** The rate in effect for `mode` in `values`: AGR's 0 is "the same as
 *  Capture FPS", as the batch reads it. */
export function rateOf(values, mode) {
  const key = rateKeyFor(mode);
  return key === 'agr_fps' ? (values.agr_fps || values.capture_fps) : values[key];
}

/** The finished-clip codec setting for `mode`, or null for AGR, whose takes
 *  are not videos and have nothing to finish. */
export function codecKeyFor(mode) {
  if (mode === 'obs') return 'finish_codec_obs';
  if (mode === 'direct_to_video') return 'finish_codec_video';
  if (mode === 'frame_sequence') return 'finish_codec_frames';
  return null;
}

/** Copies `value` deeply, so a pending edit never shares an array or object
 *  with the block or the form. */
const copyOf = (value) => (value === undefined ? undefined : JSON.parse(JSON.stringify(value)));

/**
 * The values the Project tab starts from when the page opens: the open
 * project's (`block`) over the app's (`form`, the settings file's values),
 * for every key the tab edits. `pcId` picks this PC's folders.
 */
export function startingValues(form, block, pcId) {
  const asked = Object.fromEntries(PROJECT_TAB_KEYS.map((key) => [key, copyOf(form?.[key])]));
  const values = resolveSettings(asked, block, pcId);
  return Object.fromEntries(PROJECT_TAB_KEYS.map((key) => [key, copyOf(values[key])]));
}

const RATE_KEYS = ['capture_fps', 'obs_capture_fps', 'agr_fps'];

/**
 * `pending` with each rate that the capture mode doesn't use, and that holds
 * something unusable (a box typed blank before the mode was changed), put
 * back to where the page started. pendingProblems skips those, so nothing
 * tells the user about them; Apply must not write them into the project
 * (JSON turns NaN into null) or count them as edits.
 */
export function usablePending(start, pending) {
  const used = new Set([rateKeyFor(pending.capture_mode)]);
  if (pending.capture_mode === 'agr' && pending.agr_fps === 0) used.add('capture_fps');
  const next = { ...pending };
  for (const key of RATE_KEYS) {
    const ok = isWhole(pending[key]) || (key === 'agr_fps' && pending[key] === 0);
    if (!used.has(key) && !ok) next[key] = start[key];
  }
  return next;
}

/** The keys whose pending value differs from the value the page started from. */
export function changedKeys(start, pending) {
  const usable = usablePending(start, pending);
  return PROJECT_TAB_KEYS.filter((key) => JSON.stringify(start[key]) !== JSON.stringify(usable[key]));
}

/** Whether there is anything for Apply to do. */
export function hasChanges(start, pending) {
  return changedKeys(start, pending).length > 0;
}

const isWhole = (v) => Number.isInteger(v) && v > 0;
const isSeconds = (v) => typeof v === 'number' && Number.isFinite(v) && v >= 0;

/**
 * What is wrong with the pending values, as `{ key: message }`; empty when
 * Apply can write them. Only the rate the capture mode uses is checked, so a
 * stale value under a hidden mode never blocks Apply.
 */
export function pendingProblems(pending) {
  const S = STRINGS.PROJECT_SETTINGS_PAGE;
  const problems = {};
  if (!isWhole(pending.resolution_width) || !isWhole(pending.resolution_height)) {
    problems.resolution_width = S.BAD_SIZE;
  }
  const rateKey = rateKeyFor(pending.capture_mode);
  const rate = pending[rateKey];
  // AGR's 0 means "the same as Capture FPS"; every other rate must be set.
  if (!(isWhole(rate) || (rateKey === 'agr_fps' && rate === 0))) problems[rateKey] = S.BAD_RATE;
  // AGR's 0 reads Capture FPS, which must then be usable.
  else if (rateKey === 'agr_fps' && rate === 0 && !isWhole(pending.capture_fps)) problems[rateKey] = S.BAD_RATE;
  if (!isSeconds(pending.record_start_lead)) problems.record_start_lead = S.BAD_SECONDS;
  if (!isSeconds(pending.record_stop_trail)) problems.record_stop_trail = S.BAD_SECONDS;
  const gaps = pending.kill_gaps || {};
  if (gaps.mode === 'fast_forward'
    && !(gaps.over_seconds > 0 && gaps.speed > 1 && gaps.speed <= MAX_KILL_GAP_SPEED)) {
    problems.kill_gaps = S.badKillGaps(MAX_KILL_GAP_SPEED);
  }
  const template = parseTemplate(pending.clip_name_template);
  if (template.errors.length > 0) problems.clip_name_template = template.errors.join(' ');
  return problems;
}

/**
 * The block Apply writes: `block` with each pending value that differs from
 * where the page started, every other field (and any this build doesn't
 * know) left as it was. `block` itself is not changed.
 */
export function blockWithPending(block, start, pending, pcId) {
  return followFormEdits(block, start, usablePending(start, pending), pcId).block;
}

/** A clip-name template typed into the page: blank means the default, as
 *  Configuration's field reads it. */
export function templateOrDefault(text) {
  return text && text.trim() ? text : DEFAULT_TEMPLATE;
}

/**
 * `kill_gaps` with `change` over it. A typed length or speed is kept as
 * typed, so pendingProblems can say what is wrong with it; picking Keep
 * cleans the value the way a capture reads it, since its length and speed
 * are hidden then and could not be fixed.
 */
export function killGapsWith(current, change) {
  const next = { ...(current || {}), ...change };
  return next.mode === 'fast_forward' ? next : killGapsFrom(next);
}

/** `folders` with `folder` added at the end, unless it is blank or already
 *  there (any case, either slash). */
export function withFolder(folders, folder) {
  const path = String(folder || '').trim();
  const list = [...(folders || [])];
  const key = (p) => p.replace(/\//g, '\\').replace(/[\\]+$/, '').toLowerCase();
  if (!path || list.some((p) => key(p) === key(path))) return list;
  return [...list, path];
}

/** `folders` without the one at `index`. */
export function withoutFolder(folders, index) {
  return (folders || []).filter((_, i) => i !== index);
}

/** One line per folder on a drive an earlier one already uses: a capture or
 *  export spread over them gains no room (Setup's same-drive check). */
export function sameDriveLines(folders) {
  return sameDriveFolders(folders).map((r) => STRINGS.SETUP.sameDrive(r.path, r.drive, r.first));
}

/**
 * How the project's install compares with the one picked now: 'none' (the
 * project names none), 'picked' (the same hl.exe, or the same install folder
 * name on another PC), else 'other'. Matches the install offer's rule
 * (installToOffer), which reads the same two fields.
 */
export function installState(madeWith, pickedHlExe) {
  if (!madeWith || (!madeWith.hlExe && !madeWith.name)) return 'none';
  if (madeWith.hlExe && samePath(madeWith.hlExe, pickedHlExe || '')) return 'picked';
  const picked = madeWithOf(pickedHlExe);
  return madeWith.name && picked?.name === madeWith.name ? 'picked' : 'other';
}
