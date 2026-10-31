// vegas_edl.js
// "Export VEGAS EDL" (#110): the finished renders as a CMX 3600 EDL that
// VEGAS Pro opens (File > Import > EDL Text File, or File > Open), with each
// clip one after another on the timeline, its picture on the video track and
// its sound on the audio track. Pure, so it can be tested on its own.
//
// What VEGAS 21 does with the file (measured, see the PR for #110):
//  - It finds each clip by the path after `* FROM CLIP NAME:`, so the full
//    path with the extension goes there. A bare name is searched for in
//    no folder at all and VEGAS asks for the file.
//  - It reads the timecodes at 30 frames a second, whatever the project's
//    frame rate; a frame number from 30 up is carried into the seconds.
//  - It ignores `* LOC:` locators (no markers or regions appear). They stay
//    in the file for editors that read them, with the clip's label.
//  - An EDL holds at most 999 events (CMX 3600's three-digit event number),
//    so a long list is split into several files (`planEdlParts`).
//
// A HUD render is the HUD layer of a clip already on the timeline: it gets a
// video event only (it has no sound) at the same time as that clip, so it
// lies over it instead of after it.

/** Frames a second the EDL's timecodes count in: how VEGAS reads them. */
export const EDL_TIMECODE_FPS = 30;

/** Frames in `seconds`, rounded to the EDL's own frame rate. */
export function edlFrames(seconds, fps = EDL_TIMECODE_FPS) {
  return Math.max(0, Math.round((Number(seconds) || 0) * fps));
}

/** A frame count as `hh:mm:ss:ff`. */
export function edlTimecode(frames, fps = EDL_TIMECODE_FPS) {
  const f = Math.max(0, Math.floor(frames));
  const pad = (n) => String(n).padStart(2, '0');
  const secs = Math.floor(f / fps);
  return `${pad(Math.floor(secs / 3600))}:${pad(Math.floor(secs / 60) % 60)}:${pad(secs % 60)}:${pad(f % fps)}`;
}

/** The most events one EDL file holds: CMX 3600 numbers them with three digits. */
export const EDL_MAX_EVENTS = 999;

/** The job's `stream` for a HUD-only render. */
const HUD_STREAM = 'HUD ONLY';

/** The frame rate in a job's settings line (`ProRes @ 300fps (Render
 *  Settings)`), or null when the line has none (a copy, a captured video). */
export function jobFps(job) {
  const m = /@\s*(\d+(?:\.\d+)?)\s*fps/i.exec(job?.settings_summary || '');
  return m ? Number(m[1]) : null;
}

/** A job's length from its frames and settings line, or null when that
 *  can't tell it (a copy, a captured video: their file is probed instead). */
function lengthFromSettings(job) {
  const fps = jobFps(job);
  return fps && job.frames > 0 ? job.frames / fps : null;
}

const finishedJobs = (jobs, finishedStatus) =>
  (jobs || []).filter((j) => j.status === finishedStatus && j.output_path);

/** The output files of the finished renders whose length only the file
 *  itself can tell, for the caller to probe. */
export function pathsToProbe(jobs, finishedStatus = 'Finished') {
  return finishedJobs(jobs, finishedStatus)
    .filter((job) => lengthFromSettings(job) === null)
    .map((job) => job.output_path);
}

/**
 * The clips to put on the timeline: every finished render with a file and a
 * known length, in the order the Render tab lists them. `probed` maps an
 * output path to the length read from the file, in seconds, for the ones
 * `pathsToProbe` named. `skipped` counts the finished ones whose length
 * still can't be told, so the toast can say so.
 */
export function edlClips(jobs, finishedStatus = 'Finished', probed = new Map()) {
  const clips = [];
  let skipped = 0;
  finishedJobs(jobs, finishedStatus).forEach((job) => {
    const seconds = lengthFromSettings(job) ?? probed.get(job.output_path);
    if (!(seconds > 0)) { skipped += 1; return; }
    clips.push({
      name: job.name,
      path: job.output_path,
      seconds,
      hud: job.stream === HUD_STREAM,
      takeFolder: job.take_folder || '',
    });
  });
  return { clips, skipped };
}

/** The events a clip writes: picture and sound, or the picture alone for a HUD render. */
const eventsOf = (clip) => (clip.hud ? 1 : 2);

/**
 * The clips split into files of at most `EDL_MAX_EVENTS` events each. A HUD
 * render moves up behind the clip of its own take (so `vegasEdl` can lay it
 * over that clip) and the two stay in the same file.
 */
export function planEdlParts(clips, maxEvents = EDL_MAX_EVENTS) {
  const units = [];
  const mainOfTake = new Map();
  clips.forEach((clip) => {
    const home = clip.hud && clip.takeFolder ? mainOfTake.get(clip.takeFolder) : null;
    if (home) { home.push(clip); return; }
    const unit = [clip];
    units.push(unit);
    if (!clip.hud && clip.takeFolder) mainOfTake.set(clip.takeFolder, unit);
  });
  const parts = [];
  let events = 0;
  units.forEach((unit) => {
    const size = unit.reduce((n, clip) => n + eventsOf(clip), 0);
    if (parts.length === 0 || events + size > maxEvents) {
      parts.push([]);
      events = 0;
    }
    parts[parts.length - 1].push(...unit);
    events += size;
  });
  return parts;
}

/** The files for `count` parts: `path` itself for one, else `name_part1.edl`, `name_part2.edl`... */
export function partPaths(path, count) {
  if (count <= 1) return [path];
  const dot = path.lastIndexOf('.');
  const hasExt = dot > path.lastIndexOf('\\') && dot > path.lastIndexOf('/');
  const stem = hasExt ? path.slice(0, dot) : path;
  const ext = hasExt ? path.slice(dot) : '.edl';
  return Array.from({ length: count }, (_, i) => `${stem}_part${i + 1}${ext}`);
}

const oneLine = (text) => String(text ?? '').replace(/[\r\n]+/g, ' ').trim();

/** The whole EDL, CRLF line ends. Each clip starts where the last ended. */
export function vegasEdl(clips, title = 'DoD Studio highlights') {
  const lines = [`TITLE:   ${oneLine(title)}`, 'FCM: NON-DROP FRAME', ''];
  let edit = 1;
  let at = 0;
  let lastMain = null;
  clips.forEach((clip, i) => {
    const length = Math.max(1, edlFrames(clip.seconds));
    const reel = `DOD${String(i + 1).padStart(4, '0')}`.slice(0, 8);
    const overlay = clip.hud && clip.takeFolder && lastMain?.takeFolder === clip.takeFolder;
    const start = overlay ? lastMain.start : at;
    const times = `${edlTimecode(0)} ${edlTimecode(length)} ${edlTimecode(start)} ${edlTimecode(start + length)}`;
    const tracks = clip.hud ? [['V     ', true]] : [['V     ', true], ['AA    ', false]];
    tracks.forEach(([track, isVideo]) => {
      lines.push(`${String(edit).padStart(3, '0')}  ${reel.padEnd(8)} ${track} C        ${times}`);
      lines.push(`* FROM CLIP NAME: ${oneLine(clip.path)}`);
      if (isVideo) lines.push(`* LOC: ${edlTimecode(start)} RED     ${oneLine(clip.name)}`);
      edit += 1;
    });
    if (!overlay) {
      if (!clip.hud) lastMain = { takeFolder: clip.takeFolder, start };
      at += length;
    }
  });
  return lines.join('\r\n') + '\r\n';
}
