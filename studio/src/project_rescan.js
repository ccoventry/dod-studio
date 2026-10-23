// project_rescan.js
// Opening a project whose demos an older DoD Studio scanned: counts them
// (scan_version.js) and offers to rescan them. The rescan is the queue's own
// scan (queue_scan.js), with its status line and bar, and it carries each
// highlight's work to the new scan (rescan_carry.js). "Not now" leaves them;
// nothing is saved about the answer, so the offer comes back next open.

import { outdatedDemos } from './scan_version.js';
import { themedConfirm } from './themed_confirm.js';
import { fileNameOf, folderOf, shortFolder } from './path_display.js';
import { STRINGS } from './strings.js';

/**
 * @param {object} app  What this needs from main.js:
 *   getDemos() the queue, and scan(paths) main.js's triggerAutoScan.
 */
export function createProjectRescan({ getDemos, scan }) {
  /** Resolves true when it rescanned. */
  async function offerRescan() {
    const demos = getDemos() || [];
    const outdated = outdatedDemos(demos);
    if (outdated.length === 0) return false;
    const ok = await themedConfirm(STRINGS.MAIN.outdatedDemosMessage(outdated.length, demos.length), {
      title: STRINGS.MAIN.OUTDATED_DEMOS_TITLE,
      confirmLabel: STRINGS.MAIN.OUTDATED_DEMOS_RESCAN,
      cancelLabel: STRINGS.MAIN.OUTDATED_DEMOS_NOT_NOW,
      details: outdated.map((d) => ({ primary: fileNameOf(d.path), secondary: shortFolder(folderOf(d.path)), title: d.path })),
      focusConfirm: true,
    });
    if (!ok) return false;
    await scan(outdated.map((d) => d.path));
    return true;
  }

  return { offerRescan };
}
