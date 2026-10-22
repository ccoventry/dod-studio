// project_demos.js
// A project's demos that went missing, moved, changed on disk, or turned up
// as identical copies (#21): the checks, their dialogs and the relocations.
// Moved out of main.js, which owns the queue. The queue array is reassigned
// there, so it's reached through getDemos() rather than held.

import { open } from '@tauri-apps/plugin-dialog';
import { locateMissingDemos, changedDemos } from './ipc_bridge.js';
import { renameDemoInTakeIndex } from './take_index.js';
import { themedConfirm } from './themed_confirm.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';
import { fileNameOf, folderOf, shortFolder, samePath } from './path_display.js';

/**
 * @param {object} app  What this needs from main.js:
 *   getDemos() the queue, getTakeIndex(), getScanPaths() the pinned folders,
 *   refreshQueue(changed) re-render (and mark the project unsaved when
 *   `changed`), scan(paths, opts) main.js's triggerAutoScan, and
 *   removeDemo(demo) take one row out of the queue.
 */
export function createProjectDemos({ getDemos, getTakeIndex, getScanPaths, refreshQueue, scan, removeDemo }) {
  // A loaded project's demos may have moved or been deleted since it was
  // saved (#21). Look for each missing one by its file key (size + first
  // 64 KB) near the project and its old folders, and ask before using a
  // match. Anything still missing is named in a toast. Also run by Start
  // Capture Batch over the demos with a highlight picked (no project path
  // then). Resolves to how many of `demos` are still missing.
  async function checkMissingDemos(projectPath, savedScanPaths, demos = getDemos()) {
    const parentOf = (p) => p.replace(/[\\/][^\\/]*$/, '');
    const nameOf = (p) => p.split(/[\\/]/).pop();
    // Nearest first: where each demo was, then the project's folder, then the
    // scan folders. The search is breadth-first in this order, so a demo moved
    // next door is found before a big scan folder uses up the search budget.
    // Scan entries that name single demo files aren't folders to search.
    const isDemoFile = (p) => /\.dem$/i.test(p);
    const dirs = [...new Set([
      ...demos.map((d) => parentOf(d.path)),
      projectPath && parentOf(projectPath),
      ...savedScanPaths.filter((p) => !isDemoFile(p)),
      ...getScanPaths().filter((p) => !isDemoFile(p)),
    ].filter(Boolean))];
    const missing = await locateMissingDemos(
      demos.map((d) => ({ path: d.path, file_key: d.file_key || '' })),
      dirs
    );
    // A demo marked missing earlier that is back where the queue says.
    const missingPaths = new Set(missing.map((m) => m.path));
    const back = demos.filter((d) => d.missing && !missingPaths.has(d.path));
    back.forEach((d) => {
      d.missing = false;
      d.foundAt = null;
    });
    if (missing.length === 0) {
      if (back.length > 0) refreshQueue(false);
      return 0;
    }

    // A match that is already another row's file (an identical copy) isn't
    // offered: two rows on one file would share its take records.
    missing.forEach((m) => {
      if (m.candidate && getDemos().some((d) => d.path !== m.path && samePath(d.path, m.candidate))) {
        m.candidate = null;
      }
    });
    const found = missing.filter((m) => m.candidate);
    const relocated = new Set();
    if (found.length > 0) {
      // One entry per demo: its name (or old -> new when it was renamed),
      // then the folder it's in now, shortened to the drive and the last
      // folders; the full path is the entry's hover text.
      const details = found.map((m) => {
        const oldName = nameOf(m.path);
        const newName = fileNameOf(m.candidate);
        return {
          primary: oldName === newName ? oldName : STRINGS.MAIN.relocateRenamed(oldName, newName),
          secondary: STRINGS.MAIN.relocateFolder(shortFolder(folderOf(m.candidate))),
          title: m.candidate,
        };
      });
      const ok = await themedConfirm(STRINGS.MAIN.RELOCATE_DEMOS_MESSAGE, {
        title: STRINGS.MAIN.RELOCATE_DEMOS_TITLE,
        confirmLabel: STRINGS.MAIN.RELOCATE_CONFIRM,
        cancelLabel: STRINGS.MAIN.RELOCATE_CANCEL,
        details,
        footer: STRINGS.MAIN.RELOCATE_DEMOS_QUESTION,
      });
      if (ok) {
        found.forEach((m) => {
          const demo = getDemos().find((d) => d.path === m.path);
          if (!demo) return;
          relocateDemo(demo, m.candidate);
          relocated.add(m.path);
        });
        showToast(STRINGS.MAIN.relocatedDemosToast(relocated.size), 'success');
      }
    }

    const stillMissing = missing.filter((m) => !relocated.has(m.path));
    const leftFound = [];
    stillMissing.forEach((m) => {
      const demo = getDemos().find((d) => d.path === m.path);
      if (!demo) return;
      // Runtime only: non-enumerable, so neither reaches the project file.
      // `foundAt` keeps a declined match for the row's Use found copy button.
      const runtime = { writable: true, configurable: true, enumerable: false };
      Object.defineProperty(demo, 'missing', { ...runtime, value: true });
      Object.defineProperty(demo, 'foundAt', { ...runtime, value: m.candidate || null });
      if (m.candidate) leftFound.push(demo);
    });
    refreshQueue(relocated.size > 0);
    const notFound = stillMissing.filter((m) => !m.candidate);
    if (notFound.length > 0) {
      showToast(STRINGS.MAIN.missingDemosToast(notFound.map((m) => nameOf(m.path))), 'warning', 10000);
    }
    if (leftFound.length > 0) {
      showToast(STRINGS.MAIN.leftMissingToast(leftFound.map((d) => nameOf(d.path))), 'warning', 15000, {
        action: { label: STRINGS.MAIN.USE_ALL_FOUND_COPIES, onClick: () => useFoundCopies(leftFound) },
      });
    }
    return stillMissing.length;
  }

  // Start Capture Batch's check: a demo can go missing after the queue was
  // loaded. True when every demo with a highlight picked is where the queue
  // says, after any the user chose to relocate.
  async function pickedDemosPresent() {
    const picked = getDemos().filter((d) => (d.streaks || []).some((s) => s.selected === true));
    if (picked.length === 0) return true;
    if ((await checkMissingDemos(null, [], picked)) !== 0) return false;

    // A file replaced in place (or pointed at by an older build's "Use it
    // anyway") is at the right path but isn't the demo that was scanned:
    // every highlight would record the wrong moment, and any past its end
    // would leave the game stopped at the end of the demo.
    const changed = await changedDemos(picked.filter((d) => d.file_key).map((d) => ({ path: d.path, file_key: d.file_key })));
    if (changed.length === 0) return true;
    const rescan = await themedConfirm(STRINGS.MAIN.CHANGED_DEMOS_MESSAGE, {
      title: STRINGS.MAIN.CHANGED_DEMOS_TITLE,
      confirmLabel: STRINGS.MAIN.CHANGED_DEMOS_RESCAN,
      cancelLabel: STRINGS.MAIN.RELOCATE_CANCEL_PLAIN,
      details: changed.map((p) => ({ primary: fileNameOf(p), secondary: shortFolder(folderOf(p)), title: p })),
      footer: STRINGS.MAIN.CHANGED_DEMOS_QUESTION,
    });
    if (rescan) await scan(changed);
    return false;
  }

  // Uses the matches the load-time search found for demos left as missing:
  // one row's Use found copy button, or the toast's button for all of them.
  // Each is checked again first, since the file may have moved since.
  async function useFoundCopies(demos) {
    const pending = demos.filter((d) => d.missing && d.foundAt);
    if (pending.length === 0) return;
    try {
      const matches = await locateMissingDemos(
        pending.map((d) => ({ path: d.path, file_key: d.file_key || '' })),
        [...new Set(pending.map((d) => folderOf(d.foundAt)))]
      );
      const gone = [];
      let used = 0;
      pending.forEach((demo) => {
        const match = matches.find((m) => m.path === demo.path);
        if (match && match.candidate) {
          relocateDemo(demo, match.candidate);
          used += 1;
        } else {
          gone.push(fileNameOf(demo.foundAt));
          demo.foundAt = null;
        }
      });
      refreshQueue(used > 0);
      if (used > 0) showToast(STRINGS.MAIN.relocatedDemosToast(used), 'success');
      if (gone.length > 0) showToast(STRINGS.MAIN.foundCopiesGoneToast(gone), 'warning', 8000);
    } catch (err) {
      console.error('Use found copy error:', err);
    }
  }

  // Identical copies a scan skipped (#21), in two kinds:
  // - Copies of a demo that was already queued. One the user picked by hand,
  //   or one of a queued demo whose own file is missing, is offered in that
  //   row's place: same file, so the row keeps its highlights, statuses and
  //   notes. Several copies of one row: the shortest name is offered.
  // - Copies of each other within this scan: the scan kept one (the shortest
  //   name). Picked by hand, they're listed in a dialog; from a folder, a toast.
  async function offerIdenticalCopies(copies, pickedFiles) {
    const nameOfRow = (d) => d.name || fileNameOf(d.path);
    const byName = (a, b) => fileNameOf(a.demo.path).length - fileNameOf(b.demo.path).length
      || fileNameOf(a.demo.path).localeCompare(fileNameOf(b.demo.path));
    const ofQueued = copies.filter((c) => c.queued);
    const ofEachOther = copies.filter((c) => !c.queued);

    const offered = [];
    const skipped = [];
    [...ofQueued].sort(byName).forEach((c) => {
      const entry = offered.find((o) => o.sameAs === c.sameAs);
      if (entry) {
        // Another copy of the same row: named on that row's entry.
        entry.others.push(fileNameOf(c.demo.path));
      } else if (pickedFiles || c.sameAs.missing) {
        offered.push({ ...c, others: [] });
      } else {
        skipped.push(c);
      }
    });

    if (ofEachOther.length > 0 && pickedFiles) {
      // One entry per demo added, naming the copies that weren't.
      const groups = new Map();
      ofEachOther.forEach((c) => {
        if (!groups.has(c.sameAs)) groups.set(c.sameAs, []);
        groups.get(c.sameAs).push(fileNameOf(c.demo.path));
      });
      await themedConfirm(STRINGS.MAIN.PICKED_COPIES_MESSAGE, {
        title: STRINGS.MAIN.PICKED_COPIES_TITLE,
        confirmLabel: STRINGS.MAIN.PICKED_COPIES_OK,
        hideCancel: true,
        details: [...groups].map(([kept, names]) => ({
          primary: nameOfRow(kept),
          secondary: STRINGS.MAIN.pickedCopiesSkipped(names),
          title: kept.path,
        })),
      });
    } else {
      skipped.push(...ofEachOther);
    }

    if (skipped.length > 0) {
      showToast(STRINGS.MAIN.identicalCopiesToast(skipped.map((c) => [fileNameOf(c.demo.path), nameOfRow(c.sameAs)])), 'warning', 12000);
    }
    if (offered.length === 0) return;
    const ok = await themedConfirm(STRINGS.MAIN.IDENTICAL_COPIES_MESSAGE, {
      title: STRINGS.MAIN.IDENTICAL_COPIES_TITLE,
      confirmLabel: STRINGS.MAIN.IDENTICAL_COPIES_SWITCH,
      cancelLabel: STRINGS.MAIN.IDENTICAL_COPIES_KEEP,
      details: offered.map((c) => ({
        primary: STRINGS.MAIN.relocateRenamed(nameOfRow(c.sameAs), fileNameOf(c.demo.path)),
        // The queued file hasn't moved, so not "now in": either the same
        // folder as it, or where the copy is.
        secondary: (c.sameAs.missing
          ? STRINGS.MAIN.identicalCopyQueuedMissing(shortFolder(folderOf(c.demo.path)))
          : samePath(folderOf(c.demo.path), folderOf(c.sameAs.path))
            ? STRINGS.MAIN.IDENTICAL_COPY_SAME_FOLDER
            : STRINGS.MAIN.identicalCopyFolder(shortFolder(folderOf(c.demo.path))))
          + (c.others.length ? STRINGS.MAIN.identicalCopyOthers(c.others) : ''),
        title: c.demo.path,
      })),
      footer: STRINGS.MAIN.IDENTICAL_COPIES_QUESTION,
    });
    if (!ok) return;
    offered.forEach((c) => relocateDemo(c.sameAs, c.demo.path));
    refreshQueue(true);
    showToast(STRINGS.MAIN.relocatedDemosToast(offered.length), 'success');
  }

  // Takes a demo out of the queue and scans `newPath` in its place, with its
  // own highlights: for a file that isn't the one that was scanned (#21).
  async function replaceDemoWithScan(demo, newPath) {
    removeDemo(demo);
    await scan([newPath]);
  }

  // Points a demo, its highlights and the take index at a new file (#21).
  function relocateDemo(demo, newPath) {
    const oldPath = demo.path;
    demo.path = newPath;
    // The queue shows the file's name; follow a rename (or a different file).
    if (!demo.name || demo.name === fileNameOf(oldPath)) demo.name = fileNameOf(newPath);
    if (demo.missing) demo.missing = false;
    if (demo.foundAt) demo.foundAt = null;
    (demo.streaks || []).forEach((s) => {
      if (s.source_demo === oldPath) s.source_demo = newPath;
      // The cached uid still names the old path; streakUid rebuilds it.
      if ('uid' in s) s.uid = undefined;
    });
    renameDemoInTakeIndex(getTakeIndex(), oldPath, newPath);
  }

  // A missing demo's Locate button (#21): pick the file by hand. When the
  // demo has a saved file key and the pick doesn't match it, ask first.
  async function locateDemoByHand(demo) {
    try {
      const picked = await open({
        multiple: false,
        title: STRINGS.MAIN.LOCATE_DEMO_DIALOG_TITLE,
        filters: [{ name: 'Demo', extensions: ['dem'] }],
      });
      const newPath = Array.isArray(picked) ? picked[0] : picked;
      if (!newPath) return;
      // Another row's own file: two rows on one file would share its take
      // records and capture one demo's highlights from the other's footage.
      const taken = getDemos().find((d) => d !== demo && samePath(d.path, newPath));
      if (taken) {
        showToast(STRINGS.MAIN.locateAlreadyQueued(taken.name || fileNameOf(newPath)), 'error', 10000);
        return;
      }
      if (demo.file_key) {
        const parent = newPath.replace(/[\\/][^\\/]*$/, '');
        const [match] = await locateMissingDemos([{ path: demo.path, file_key: demo.file_key }], [parent]);
        if (match && !samePath(match.candidate, newPath)) {
          // A different file: this row's highlights are frame numbers in the
          // old one and won't line up, so the only useful thing is to scan it
          // fresh in this row's place.
          const ok = await themedConfirm(STRINGS.MAIN.locateMismatchMessage(demo.name || fileNameOf(demo.path), fileNameOf(newPath)), {
            title: STRINGS.MAIN.LOCATE_MISMATCH_TITLE,
            confirmLabel: STRINGS.MAIN.LOCATE_MISMATCH_CONFIRM,
          });
          if (ok) await replaceDemoWithScan(demo, newPath);
          return;
        }
      }
      relocateDemo(demo, newPath);
      refreshQueue(true);
      showToast(STRINGS.MAIN.relocatedDemosToast(1), 'success');
    } catch (err) {
      console.error('Locate demo error:', err);
    }
  }

  return {
    checkMissingDemos,
    pickedDemosPresent,
    useFoundCopies,
    offerIdenticalCopies,
    locateDemoByHand,
  };
}
