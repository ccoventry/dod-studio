// strings/auditor.js: the app's text for the Demo Auditor and the demo renamer.
// Part of the STRINGS catalog that ../strings.js assembles (#604).

export const AUDITOR_STRINGS = {
  AUDITOR: {
    PANEL_TITLE: 'Demo Auditor',
    TARGET_FOLDER_LABEL: 'Target Folder:',
    TARGET_FOLDER_PLACEHOLDER: 'Folder of demos...',
    TAB_DUPLICATES: 'Duplicates',
    TAB_SPLIT: 'Split Maps',
    TAB_RENAME: 'Rename Demos',
    BROWSE_BUTTON: 'Browse',
    START_AUDIT_BUTTON: 'Start Audit',
    CANCEL_SCAN_BUTTON: 'Cancel Scan',
    STATUS_READY: 'Status: Ready to audit',
    RESULTS_TITLE: 'Duplicate Groups Found',
    DELETE_SELECTED_BUTTON: 'Move to Recycle Bin',
    OPEN_RECYCLE_BIN_BUTTON: 'Open Recycle Bin',
    OPEN_RECYCLE_BIN_TITLE: 'Open Windows’ Recycle Bin, to see or restore the demos moved there',
    TABLE_HEADER_STATUS: 'Status',
    TABLE_HEADER_SIZE: 'Size',
    TABLE_HEADER_FILE_PATH: 'File Path',
    TABLE_HEADER_ACTION: 'Action',
    TABLE_EMPTY: 'Choose a folder and run audit to find duplicate demo files.',
    FOOTER_DEFAULT: 'Duplicates Found: 0 | Wasted Space: 0.0 MB',
    footerSummary: (count, size) => `Duplicates Found: ${count} | Wasted Space: ${size}`,

    SELECT_FOLDER_DIALOG_TITLE: 'Select Folder to Audit',
    // Duplicates' "Folders to check" (audit_folders.js, #844).
    FOLDERS_TO_CHECK_LABEL: 'Folders to check:',
    ADD_FOLDER_BUTTON: 'Add Folder…',
    ADD_FOLDER_DIALOG_TITLE: 'Add Folders to Check for Duplicates',
    REMOVE_FOLDER_BUTTON: 'Remove',
    NO_FOLDERS_YET: 'No folders yet: add a folder, or tick All drives.',
    ALL_DRIVES_LABEL: 'All drives',
    allDrivesLabel: (letters) => (letters.length ? `All drives (${letters.join(', ')})` : 'All drives'),
    ALL_DRIVES_TITLE: "Check every fixed drive in one pass, so a copy on one drive pairs with its original on another. USB sticks and network drives are left out, and so are the Recycle Bin and System Volume Information.",
    // "Show demos backed up in archives" (#929).
    LOOK_IN_ZIPS_LABEL: 'Show demos backed up in archives (.zip, .7z, .rar)',
    LOOK_IN_ZIPS_TITLE: "Also read the demos inside .zip, .7z and .rar files, and mark each loose demo that an archive already holds as backed up, with every archive that holds it. An archive is a backup: a loose demo is never offered for deletion because an archive holds it, and archives are only read, never changed. Only .dem files count, an archive inside an archive isn't opened, and encrypted or damaged ones are skipped (the activity log names them). The first audit of big archives takes longer, solid .7z and .rar ones most; later audits remember them until they change.",
    // A loose demo an archive holds a copy of (#929).
    backedUpChip: (archives) => (archives.length === 1
      ? `backed up in ${archives[0]}`
      : `backed up in ${archives.length} archives`),
    backedUpTitle: (entries) => `A copy of this demo is in:\n${entries.join('\n')}\n\nArchives are only read, never changed.`,
    backedUpSectionLabel: (expanded, count) => `${expanded ? '▼' : '▶'} Backed up in archives (${count} demo${count === 1 ? '' : 's'})`,
    BACKED_UP_SECTION_NOTE: 'Each of these demos has a copy in an archive. Nothing here is deleted.',
    footerBackedUp: (count) => ` | Backed up in archives: ${count}`,
    CHOOSE_FOLDER_FIRST: 'Add a folder to check, or tick All drives, before starting an audit.',
    AUDITING_IN_PROGRESS_ROW: 'Auditing in progress...',
    auditFailedRow: (e) => `Audit failed: ${e}`,
    CANCELLING_STATUS: 'Cancelling…',
    CANCELLED_STATUS: 'Cancelled. Nothing was listed.',
    CANCELLED_ROW: 'The audit was cancelled before it finished, so no groups are listed.',
    // The progress line (audit_duplicates.js): listing folders, then keying.
    listingLine: (folders) => `Looking for demos: ${folders} folder${folders === 1 ? '' : 's'} read`,
    listingLineOf: (folders, last) => `Looking for demos: ${folders} folder${folders === 1 ? '' : 's'} read (about ${last} last time)`,
    keyingLine: (done, total) => `Comparing demos: ${done} / ${total}`,
    zipsLine: (done, total) => `Looking inside archives: ${done} / ${total}`,
    checkingCopiesLine: (done, total) => `Checking each copy matches the kept one byte for byte: ${done} / ${total}`,
    deletingLine: (done, total, permanent) => (permanent
      ? `Deleting permanently: ${done} / ${total}`
      : `Moving to the Recycle Bin: ${done} / ${total}`),
    doneStatus: (demos, groups, elapsed, archiveDemos = 0) => `Done in ${elapsed}: ${demos} demo${demos === 1 ? '' : 's'} checked${archiveDemos > 0 ? ` and ${archiveDemos} inside archives` : ''}, ${groups} with copies.`,
    NO_DUPLICATES_ROW: 'No duplicates found! Your demos are clean.',
    groupToggleLabel: (expanded, count) => `${expanded ? '▼' : '▶'} Group (${count} files)`,
    identicalHash: (hash) => `Identical Hash: ${hash}`,
    ORIGINAL_FILE_TITLE: 'The copy this group keeps. Pick another copy to keep that one instead.',
    KEEP_THIS_TITLE: 'Keep this copy instead. The one kept now becomes a copy you can delete.',
    KEEP_CHANGED_NOTE: 'Kept copy changed by you',
    NOT_A_COPY_LABEL: '   ≠ Not an exact copy',
    NOT_A_COPY_TITLE: "Compared byte for byte with the kept copy, and it's different: another demo that only starts the same way. It isn't deleted, recycled or counted as a copy.",
    KEEP_RESET_BUTTON: 'Reset',
    keepResetTitle: (path) => `Keep the copy the audit picked again: ${path}`,
    PREFER_FOLDER_BUTTON: 'Keep copies already in a folder…',
    PREFER_FOLDER_TITLE: 'Moves nothing. Pick a folder: every group that already has a copy in it (or in a folder inside it) keeps that copy, so a long list needs no picking row by row.',
    PREFER_FOLDER_DIALOG_TITLE: 'Keep the copies that are already in this folder',
    preferFolderToast: (found, changed, folder) => (found
      ? `Keeping the copy in ${folder} in ${found} group${found === 1 ? '' : 's'} (${changed} changed).`
      : `No group has a copy in ${folder}. This only picks which copy to keep; it doesn't move demos.`),
    IN_USE_TITLE: "The game or this project reads this copy, so it isn't ticked. Tick it only if you're sure.",
    // Why a copy is kept or left unticked (audit_manager.rs's InUse).
    IN_USE: { game_folder: 'in the game folder', queue: "in this project's queue" },
    KEPT_LABEL: 'Kept',
    keptBecause: (why) => `Kept (${why})`,
    COPY_LABEL: '   ↳ Copy',
    inUseCopy: (why) => `   ↳ Copy, ${why}`,
    COPY_PATH_BUTTON: '📋 Copy Path',
    OPEN_FOLDER_BUTTON: '📁 Open Folder',
    PATH_COPIED_TOAST: 'Path copied to clipboard.',
    COPY_PATH_FAILED_TOAST: 'Failed to copy path.',
    DELETE_PERMANENT_BUTTON: 'Delete Permanently…',
    recycleNSelectedFiles: (n) => `Move ${n} to Recycle Bin`,
    deleteNPermanently: (n) => `Delete ${n} Permanently…`,
    recycleConfirm: (n) => `Move ${n} duplicate file${n === 1 ? '' : 's'} to the Recycle Bin? Each is first compared byte for byte with the copy kept, and any that differ stay where they are. You can put them back from the Recycle Bin.`,
    deletePermanentlyConfirm: (n) => `Delete ${n} duplicate file${n === 1 ? '' : 's'} permanently? They skip the Recycle Bin and can't be put back.`,
    recycledFilesToast: (n) => `Moved ${n} duplicate file${n === 1 ? '' : 's'} to the Recycle Bin.`,
    deletedFilesToast: (n) => `Deleted ${n} duplicate file${n === 1 ? '' : 's'} permanently.`,
    someNotDeletedToast: (n, first) => `${n} file${n === 1 ? ' was' : 's were'} not deleted. ${first}`,
    deletedStatus: (deleted, failed, permanent) =>
      `${permanent ? 'Deleted' : 'Moved to the Recycle Bin'}: ${deleted}${failed ? `, ${failed} not deleted` : ''}.`,
    deletionFailedToast: (e) => `Deletion failed: ${e}`,
    megabytesLabel: (mb) => `${mb} MB`,

    // Consolidate (#872): audit_consolidate.js and audit_consolidate_ui.js.
    CONSOLIDATE_BUTTON: 'Consolidate…',
    CONSOLIDATE_TITLE: 'Move every demo the audit found into one folder. Only the kept copy of each group moves; the other copies go to the Recycle Bin once each matches it byte for byte.',
    CONSOLIDATE_DEST_LABEL: 'Move every demo into:',
    CONSOLIDATE_DEST_PLACEHOLDER: 'Choose a folder…',
    CONSOLIDATE_BROWSE_BUTTON: 'Choose…',
    CONSOLIDATE_DIALOG_TITLE: 'Folder to move every demo into',
    CONSOLIDATE_LAYOUT_FLAT: 'All in one folder',
    CONSOLIDATE_LAYOUT_SUBFOLDERS: 'Keep their subfolders',
    CONSOLIDATE_LAYOUT_SUBFOLDERS_TITLE: 'Each demo keeps the folders it was in below the folder the audit checked, recreated under the destination.',
    CONSOLIDATE_GAME_FOLDERS: 'Also move demos in game folders',
    CONSOLIDATE_GAME_FOLDERS_TITLE: "Demos inside an install's dod folder stay there unless this is ticked, because the game plays demos from there. Ticked, their copies elsewhere can be recycled too.",
    CONSOLIDATE_START_BUTTON: 'Move Demos',
    CONSOLIDATE_CANCEL_BUTTON: 'Cancel',
    CONSOLIDATE_UNDO_BUTTON: 'Undo Last Consolidate',
    consolidateUndoTitle: (n, dest) => `Put the ${n} demo${n === 1 ? '' : 's'} moved into ${dest} back where they were. Copies sent to the Recycle Bin stay there.`,
    CONSOLIDATE_NEEDS_AUDIT: 'Run an audit first: Consolidate moves the demos it found.',
    CONSOLIDATE_NEEDS_DEST: 'Choose the folder to move the demos into.',
    CONSOLIDATE_PLANNING: 'Working out what moves…',
    consolidatePlanFailed: (e) => `Can't consolidate: ${e}`,
    consolidatePreview: (p, size) => {
      const parts = [`Move ${p.moves.length} demo${p.moves.length === 1 ? '' : 's'} (${size}) into ${p.destination}`];
      if (p.recycle.length) {
        const n = p.recycle.length;
        parts.push(`recycle ${n} cop${n === 1 ? 'y' : 'ies'} (each first checked byte for byte against the demo kept)`);
      }
      if (p.renamed) parts.push(`rename ${p.renamed}`);
      let text = parts.join(', ') + '.';
      if (p.already_there) text += ` ${p.already_there} already there.`;
      const inGame = p.stay_in_game_folders + p.copies_in_game_folders;
      if (inGame) text += ` ${inGame} in game folders left where they are.`;
      return text;
    },
    NOTHING_TO_CONSOLIDATE: 'Nothing to move: every demo is already there or in a game folder.',
    consolidateNoSpace: (need, free) => `Not enough space: the demos copied from other drives need ${need} and ${free} is free.`,
    CONSOLIDATE_CONFIRM_TITLE: 'Consolidate Demos',
    CONSOLIDATE_CONFIRM_FOOTER: 'Undo puts the moved demos back. Copies in the Recycle Bin can be restored from there. Other project files that list a moved demo will ask you to locate it.',
    consolidateConfirmLabel: (n) => `Move ${n} Demo${n === 1 ? '' : 's'}`,
    movingLine: (done, total, bytes, totalBytes) => `Moving demos: ${done} / ${total} (${bytes} of ${totalBytes})`,
    recyclingLine: (done, total) => `Moving copies to the Recycle Bin: ${done} / ${total}`,
    undoingLine: (done, total, bytes, totalBytes) => `Putting demos back: ${done} / ${total} (${bytes} of ${totalBytes})`,
    CONSOLIDATE_CANCELLING: 'Cancelling after the current demo…',
    consolidatedStatus: (o, queued) => {
      let text = `Moved ${o.moved.length} demo${o.moved.length === 1 ? '' : 's'}`;
      if (o.recycled.length) text += `, recycled ${o.recycled.length} cop${o.recycled.length === 1 ? 'y' : 'ies'}`;
      text += '.';
      if (o.cancelled) text += ' Cancelled: the rest stayed where they were, and no copies were recycled.';
      const notCopies = o.recycle_failed.filter((f) => f.not_a_copy).length;
      if (notCopies) text += ` ${notCopies} cop${notCopies === 1 ? 'y was' : 'ies were'} kept: not an exact copy of the demo kept.`;
      const failed = o.failed.length + o.recycle_failed.length - notCopies;
      if (failed) text += ` ${failed} failed.`;
      if (queued) text += ` ${queued} in this project's queue updated.`;
      return text;
    },
    undoneStatus: (o, queued) => `Put back ${o.moved.length} demo${o.moved.length === 1 ? '' : 's'}.`
      + (o.cancelled ? ' Cancelled: Undo again to put back the rest.' : '')
      + (o.failed.length ? ` ${o.failed.length} could not be put back.` : '')
      + (queued ? ` ${queued} in this project's queue updated.` : ''),
    consolidateFailedToast: (n, first) => `${n} demo${n === 1 ? ' was' : 's were'} not moved or recycled. ${first}`,
    CONSOLIDATE_UNDO_CONFIRM: 'Put the demos from the last Consolidate back where they were?',
    CONSOLIDATED_ROW: 'Demos moved. Run the audit again to check the folder.',
  },
  // #441: clip names, their template and its checks.
  // ── demo_rename_ui.js: the Demo Auditor's renamer (#469) ─────────────────
  DEMO_RENAME: {
    PANEL_TITLE: 'Rename Demos',
    HINT: "Renames the demos in the Target Folder above, subfolders too, in place. Each type has its own template. New names keep letters, digits, - and _ only: anything else becomes _. Nothing changes until you click Rename, and Undo puts the last batch back.",
    POV_LABEL: 'POV demos:',
    HLTV_LABEL: 'HLTV demos:',
    RESET_BUTTON: 'Default',
    RESET_TITLE: 'Go back to the default template.',
    LOWERCASE_LABEL: 'Lowercase the whole name',
    LOWERCASE_TITLE: 'Every new name in lower case, both templates, without :lower on each placeholder.',
    LIST_BUTTON: 'List Demos',
    LIST_TITLE: "Read every demo in the Target Folder. Demos the analyzer hasn't seen take about a second each.",
    CANCEL_BUTTON: 'Cancel',
    APPLY_BUTTON: 'Rename',
    applyButton: (count) => `Rename ${count} Demo${count === 1 ? '' : 's'}`,
    UNDO_BUTTON: 'Undo Last Rename',
    undoTitle: (count, when) => `Put back the ${count} demo${count === 1 ? '' : 's'} renamed ${when}.`,
    NOTHING_TO_UNDO: 'No rename to undo.',
    COL_CURRENT: 'Current Name',
    COL_NEW: 'New Name',
    COL_TYPE: 'Type',
    TABLE_EMPTY: 'Choose a Target Folder above and click List Demos.',
    NO_DEMOS: 'No demos in this folder.',
    CHOOSE_FOLDER_FIRST: 'Choose a Target Folder first.',
    reading: (done, total) => `Reading demos: ${done} / ${total}`,
    listed: (count, renames) => `${count} demo${count === 1 ? '' : 's'}, ${renames} to rename.`,
    footer: (count, renames) => `Demos listed: ${count} | To rename: ${renames}`,
    CANCELLED: 'Stopped. The demos read so far are listed.',
    STATUS_SAME: 'already named this way',
    STATUS_SKIPPED: 'not ticked',
    statusUnreadable: (error) => `couldn't be read: ${error}`,
    STATUS_TEMPLATE: 'its template has a problem (see above)',
    NUMBERED_TITLE: 'Another demo in this folder already has or gets this name, so a number is added.',
    confirmTitle: (count) => `Rename ${count} demo${count === 1 ? '' : 's'}?`,
    CONFIRM_MESSAGE: 'They keep their contents and dates, so projects can find them again. Undo Last Rename puts them back.',
    CONFIRM_OK: 'Rename',
    renamedToast: (count) => `Renamed ${count} demo${count === 1 ? '' : 's'}.`,
    undoneToast: (count) => `Put back ${count} demo${count === 1 ? '' : 's'}.`,
    failedToast: (count, first) => `${count} couldn't be renamed. The first: ${first}`,
    // Rename and Undo's progress line (#958).
    renamingStart: (count) => `Renaming ${count} demo${count === 1 ? '' : 's'}…`,
    renaming: (done, total) => `Renaming ${done} of ${total}…`,
    UNDO_STARTING: 'Putting back the last renames…',
    puttingBack: (done, total) => `Putting back ${done} of ${total}…`,
    renamedIn: (count, elapsed) => `Renamed ${count} demo${count === 1 ? '' : 's'} in ${elapsed}.`,
    putBackIn: (count, elapsed) => `Put back ${count} demo${count === 1 ? '' : 's'} in ${elapsed}.`,
    notRenamed: (count) => ` ${count} couldn't be renamed.`,
    povOnly: (raw) => `${raw}: an HLTV demo has no recording player, so it only works in the POV template.`,
    // Rename Demos' own meaning of a placeholder, where it isn't the one in
    // PLACEHOLDERS.DESCRIPTIONS.
    DESCRIPTIONS: {
      name: "The recording player's name without their team's tag, or their Use as name from the Players tab",
      full_name: "The recording player's name as it is, tag included, or their Use as name from the Players tab",
      kills: "The recording player's kills in the whole demo (a clip name's {kills} is the highlight's kills instead)",
      deaths: "The recording player's deaths",
      faction: "The recording player's side: Allies, British or Axis",
      enemy_faction: 'The other side',
      team_name: "The recording player's team: its display name from the project's Teams list, or unknown when their side has no tag",
      opponent: "The other team: its display name from the project's Teams list, or unknown when that side has no tag",
      demo: 'The current file name, without .dem',
      half: 'Which half of its match the demo records, h1 or h2, told from the players\' sides and the chat across the demos listed together. '
        + 'When nothing says, it gives unknown or your word after |, never a guess',
      match: 'The match\'s number among the demos listed together, by date. Every demo of one match gets the same number: the HLTV demo and each POV of both halves',
    },
    chipTitle: (description, value) => (value ? `${description}. First demo: ${value}` : description),
  },
};
