// demo_menu_items.js
// Which items a Master Demo Queue row's right-click menu shows, and which are
// dimmed (#1111). Pure, so it is tested without a page; demo_context_menu.js
// supplies what each item runs.

import { STRINGS } from './strings.js';

/**
 * The menu's items for one demo, top to bottom: open / play, edit, copy, then
 * remove last. `a` holds what each item runs; an entry that is null is an
 * action this demo has no use for, so its item is left out, while one whose
 * precondition isn't met is dimmed with the reason.
 */
export function demoMenuItems(demo, a) {
  const S = STRINGS.CONTEXT_MENU;
  const gone = demo.missing ? S.DEMO_MISSING : false;
  return [
    { label: S.LAUNCH_PREVIEW, onSelect: a.launchPreview, disabled: gone },
    { label: S.REVIEW, onSelect: a.review, disabled: gone },
    { label: S.OPEN_IN_ANALYZER, onSelect: a.openInAnalyzer, disabled: gone },
    { label: S.OPEN_FOLDER, onSelect: a.openFolder, disabled: gone },
    { separator: true },
    // `a.splitBlocked`: why the row's split can't run now (already split, or
    // splitting), so a second split doesn't write copies of the pieces.
    a.split && { label: S.SPLIT_MAPS, onSelect: a.split, disabled: a.splitBlocked || false },
    a.useFound && { label: S.USE_FOUND_COPY, onSelect: a.useFound },
    a.locate && { label: S.LOCATE, onSelect: a.locate },
    { separator: true },
    { label: S.COPY_NAME, onSelect: a.copyName },
    { label: S.COPY_PATH, onSelect: a.copyPath },
    { separator: true },
    { label: S.REMOVE, onSelect: a.remove, danger: true },
  ];
}
