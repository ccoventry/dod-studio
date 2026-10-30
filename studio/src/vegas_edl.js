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

/** The frame rate in a job's settings line (`ProRes @ 300fps (Render
 *  Settings)`), or null when the line has none (a copy, a captured video). */
export function jobFps(job) {
  const m = /@\s*(\d+(?:\.\d+)?)\s*fps/i.exec(job?.settings_summary || '');
  return m ? Number(m[1]) : null;
}

/**
 * The clips to put on the timeline: every finished render with a file and a
 * known length, in the order the Render tab lists them. `skipped` counts the
 * finished ones whose length can't be told, so the toast can say so.
 */
export function edlClips(jobs, finishedStatus = 'Finished') {
  const clips = [];
  let skipped = 0;
  (jobs || []).filter((j) => j.status === finishedStatus && j.output_path).forEach((job) => {
    const fps = jobFps(job);
    if (!fps || !(job.frames > 0)) { skipped += 1; return; }
    clips.push({ name: job.name, path: job.output_path, seconds: job.frames / fps });
  });
  return { clips, skipped };
}

const oneLine = (text) => String(text ?? '').replace(/[\r\n]+/g, ' ').trim();

/** The whole EDL, CRLF line ends. Each clip starts where the last ended. */
export function vegasEdl(clips, title = 'DoD Studio highlights') {
  const lines = [`TITLE:   ${oneLine(title)}`, 'FCM: NON-DROP FRAME', ''];
  let edit = 1;
  let at = 0;
  clips.forEach((clip, i) => {
    const length = Math.max(1, edlFrames(clip.seconds));
    const reel = `DOD${String(i + 1).padStart(4, '0')}`.slice(0, 8);
    const times = `${edlTimecode(0)} ${edlTimecode(length)} ${edlTimecode(at)} ${edlTimecode(at + length)}`;
    [['V     ', true], ['AA    ', false]].forEach(([track, isVideo]) => {
      lines.push(`${String(edit).padStart(3, '0')}  ${reel.padEnd(8)} ${track} C        ${times}`);
      lines.push(`* FROM CLIP NAME: ${oneLine(clip.path)}`);
      if (isVideo) lines.push(`* LOC: ${edlTimecode(at)} RED     ${oneLine(clip.name)}`);
      edit += 1;
    });
    at += length;
  });
  return lines.join('\r\n') + '\r\n';
}
