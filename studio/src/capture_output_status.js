// capture_output_status.js
// The status chip at the end of each Capture Output row (#25): whether the
// folder exists, will be created, or sits on a drive that is not there. The
// answers come from diagnoseCaptureOutputPaths, the same check the launch
// guard's banner uses, so the row and the banner never disagree.

import { STRINGS } from './strings.js';

const S = STRINGS.CAPTURE_CONFIG;

/**
 * What to show for one diagnostic ({ path, status, usable }) from
 * diagnose_capture_output_paths. `tone` is the chip colour: ok, note or bad.
 * With no diagnostic yet the chip says it is checking.
 */
export function rowStatusFor(diagnostic) {
  if (!diagnostic) return { tone: 'note', text: S.OUTPUT_ROW_CHECKING, title: S.OUTPUT_ROW_CHECKING_TITLE };
  switch (diagnostic.status) {
    case 'ok':
      return { tone: 'ok', text: S.OUTPUT_ROW_OK, title: S.OUTPUT_ROW_OK_TITLE };
    case 'not_found':
      // Same split the launch guard makes: a real drive means the folder is
      // simply created on first write.
      return diagnostic.usable
        ? { tone: 'note', text: S.OUTPUT_ROW_WILL_BE_CREATED, title: S.OUTPUT_ROW_WILL_BE_CREATED_TITLE }
        : { tone: 'bad', text: S.OUTPUT_ROW_DRIVE_MISSING, title: S.OUTPUT_ROW_DRIVE_MISSING_TITLE };
    case 'not_absolute':
      return { tone: 'bad', text: S.OUTPUT_ROW_NOT_FULL_PATH, title: S.OUTPUT_ROW_NOT_FULL_PATH_TITLE };
    case 'not_a_directory':
      return { tone: 'bad', text: S.OUTPUT_ROW_NOT_A_FOLDER, title: S.OUTPUT_ROW_NOT_A_FOLDER_TITLE };
    default:
      return { tone: 'bad', text: S.OUTPUT_ROW_MALFORMED, title: S.OUTPUT_ROW_MALFORMED_TITLE };
  }
}

/**
 * @param {object} app
 * @param {() => string[]} app.getPaths  The Capture Output list, read fresh.
 * @param {(paths: string[]) => Promise<Array<{path: string, status: string, usable: boolean}>>} app.diagnose
 * @param {Element} app.container  The list's element; chips are repainted in
 *   place, never by rebuilding the rows, so a field being typed in keeps focus.
 * @param {(message: string) => void} [app.log]  Activity log.
 */
export function createCaptureOutputStatus({ getPaths, diagnose, container, log }) {
  // Answers by path, so a rebuild (reorder, add) shows the chips at once
  // instead of flashing "Checking…". null = the check itself failed: no chip.
  const known = new Map();
  let generation = 0;

  function paintChip(chip, path) {
    const answer = known.get(path);
    chip.hidden = answer === null;
    if (answer === null) return;
    const { tone, text, title } = rowStatusFor(answer);
    chip.className = `output-row-status output-row-status-${tone}`;
    chip.textContent = text;
    chip.title = title;
  }

  /** Called by the list editor for each row it builds. */
  function decorateRow(row, item) {
    const chip = document.createElement('span');
    paintChip(chip, item);
    row.appendChild(chip);
  }

  function paintAll() {
    const paths = getPaths();
    [...container.querySelectorAll('.list-editor-row')].forEach((row, i) => {
      const chip = row.querySelector('.output-row-status');
      if (chip) paintChip(chip, paths[i]);
    });
  }

  /** Checks every path again. Safe to call on each change: only the newest
   *  run draws its answers. */
  async function refresh() {
    const paths = getPaths().filter(Boolean);
    const run = ++generation;
    // A path not answered for yet shows as checking until its answer arrives
    // (a cloud drive can take seconds); unchanged paths keep their chip.
    for (const p of [...known.keys()]) if (!paths.includes(p)) known.delete(p);
    paintAll();
    if (paths.length === 0) return;
    const started = performance.now();
    try {
      const diagnostics = await diagnose(paths);
      if (run !== generation) return;
      for (const d of diagnostics) known.set(d.path, d);
      const bad = diagnostics.filter((d) => rowStatusFor(d).tone === 'bad').length;
      log?.(`Capture Output: checked ${paths.length} folder(s), ${bad} with a problem, in ${Math.round(performance.now() - started)} ms`);
    } catch (err) {
      if (run !== generation) return;
      for (const p of paths) known.set(p, null);
      log?.(`Capture Output: could not check the folders: ${err}`);
    }
    paintAll();
  }

  return { decorateRow, refresh };
}
