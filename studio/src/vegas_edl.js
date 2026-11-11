// vegas_edl.js
// "Export VEGAS EDL" (#110): the finished renders as VEGAS Pro's own EDL text
// format (the file File > Export > EDL Text File writes, imported with File >
// Import > EDL Text File), each clip one after another on the timeline, its
// picture on a video track and its sound on an audio track. Pure, so it can be
// tested on its own.
//
// Why not CMX 3600: VEGAS 21 imported a CMX EDL with every picture event on
// ONE video track, so a HUD-only render landed on the same track as its main
// clip and covered it (clips on one track don't composite). Its own format
// carries a track number per event, which CMX has no column for.
//
// What VEGAS does with this format (measured on an export of VEGAS's own, see
// the PR for #1251):
//  - One row per event, `; `-separated, the header line first, CRLF line ends.
//    Times are milliseconds with four decimals. The path is quoted.
//  - Video rows carry Channels 0, audio rows Channels 2.
//  - VEGAS writes the topmost video track as Track 1, the next as Track 2 and
//    the audio track as Track 0. Numbering the tracks 0/1/2 instead put all the
//    video on one track and made empty audio tracks, so keep VEGAS's own.
//  - It lists the rows by track: Track 1's, then Track 2's, then Track 0's.
//
// A HUD render is the HUD layer of a clip already on the timeline: it gets a
// video row only (it has no sound), on the track above, at the start and
// length of that clip, so the footage shows through its transparent parts.

/** The save dialog's default name and filter: `.txt`, because VEGAS Pro's
 *  File > Import > EDL Text File dialog only lists `*.txt` files. */
export const EDL_DEFAULT_FILE_NAME = 'dod_highlights.txt';
export const EDL_SAVE_FILTER = { name: 'VEGAS EDL (*.txt)', extensions: ['txt'] };

/**
 * Where the save dialog opens: the folder holding the exported clips, with the
 * default name. The nameplate script looks for the clip list in a clip's own
 * folder and the one above, so a save there needs no question (#429). When the
 * clips span folders, the one holding most of them (a tie goes to the earliest
 * clip's); no clips, or clips without a folder, give the bare name.
 */
export function edlDefaultPath(clips) {
  const counts = new Map();
  for (const clip of clips || []) {
    const path = String(clip?.path || '');
    const cut = Math.max(path.lastIndexOf('\\'),path.lastIndexOf('/'));
    if (cut <= 0) continue;
    const folder = path.slice(0, cut);
    const entry = counts.get(folder) || { n: 0, sep: path[cut] };
    entry.n += 1;
    counts.set(folder, entry);
  }
  let best = null;
  for (const [folder, { n, sep }] of counts) {
    if (!best || n > best.n) best = { folder, n, sep };
  }
  return best ? `${best.folder}${best.sep}${EDL_DEFAULT_FILE_NAME}` : EDL_DEFAULT_FILE_NAME;
}

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
    // take_key: whose highlight it is, for the clip manifest (#429).
    clips.push({
      name: job.name,
      path: job.output_path,
      seconds,
      take_key: job.take_key || '',
      hud: job.stream === HUD_STREAM,
      takeFolder: job.take_folder || '',
    });
  });
  return { clips, skipped };
}

/** VEGAS's own track numbers (see the header): the upper video track, the
 *  lower one, and the audio track. */
const TRACK_UPPER = 1;
const TRACK_LOWER = 2;
const TRACK_AUDIO = 0;

const COLUMNS = [
  'ID', 'Track', 'StartTime', 'Length', 'PlayRate', 'Locked', 'Normalized', 'StretchMethod', 'Looped',
  'OnRuler', 'MediaType', 'FileName', 'Stream', 'StreamStart', 'StreamLength', 'FadeTimeIn',
  'FadeTimeOut', 'SustainGain', 'CurveIn', 'GainIn', 'CurveOut', 'GainOut', 'Layer', 'Color',
  'CurveInR', 'CurveOutR', 'PlayPitch', 'LockPitch', 'FirstChannel', 'Channels',
];

/** Seconds as milliseconds, the way VEGAS writes them: four decimals. */
export const edlMs = (seconds) => (Math.max(0, Number(seconds) || 0) * 1000).toFixed(4);

/** One row; the renders already carry their speed, so PlayRate is 1. */
function row(id, track, type, path, startMs, lengthMs) {
  return [
    id, track, startMs, lengthMs, '1.000000', 'FALSE', 'FALSE', 0, 'TRUE', 'FALSE', type,
    `"${String(path ?? '').replace(/[\r\n]+/g, ' ')}"`, 0, '0.0000', lengthMs, '0.0000', '0.0000',
    '1.000000', 4, '0.000000', 4, '0.000000', 0, -1, 4, 4, '0.000000', 'FALSE', 0,
    type === 'AUDIO' ? 2 : 0,
  ].join('; ');
}

/**
 * The whole EDL, CRLF line ends. Each main clip starts where the last ended: a
 * video row and an audio row. A HUD render is a video row on the upper track
 * at its take's main clip (the latest one of the same take folder), with that
 * clip's length. With no HUD render in the export, the main video takes the
 * upper track, so the timeline has no empty track above it. A HUD with no main
 * clip of its take is placed after the rest, still on the upper track.
 */
export function vegasEdl(clips) {
  const mainTrack = clips.some((clip) => clip.hud) ? TRACK_LOWER : TRACK_UPPER;
  const upper = [];
  const lower = [];
  const audio = [];
  let at = 0;
  let lastMain = null;
  clips.forEach((clip) => {
    const overlay = clip.hud && clip.takeFolder && lastMain?.takeFolder === clip.takeFolder;
    const start = (overlay ? lastMain.start : at).toFixed(4);
    const length = (overlay ? lastMain.length : Number(edlMs(clip.seconds))).toFixed(4);
    if (clip.hud) {
      upper.push((id) => row(id, TRACK_UPPER, 'VIDEO', clip.path, start, length));
    } else {
      (mainTrack === TRACK_UPPER ? upper : lower).push((id) => row(id, mainTrack, 'VIDEO', clip.path, start, length));
      audio.push((id) => row(id, TRACK_AUDIO, 'AUDIO', clip.path, start, length));
      lastMain = { takeFolder: clip.takeFolder, start: Number(start), length: Number(length) };
    }
    if (!overlay) at += Number(length);
  });
  const rows = [...upper, ...lower, ...audio].map((make, i) => make(i + 1));
  const header = COLUMNS.map((c) => `"${c}"`).join(';');
  return [header, ...rows].join('\r\n') + '\r\n';
}
