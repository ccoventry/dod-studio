// project_file_type.js
// Project files are `.dodstudio` (#528): JSON inside, as before, under a name
// Windows opens with DoD Studio. Projects saved as `.json` before that keep
// loading, and saving one writes back to that same `.json`
// (project_session.js keeps the loaded path), so nothing is renamed behind
// the user's back: only when they take the offer to switch it, once it has
// loaded (project_convert.js). A project's first save picks the name, and it
// defaults to `.dodstudio`.

import { STRINGS } from './strings.js';
import { fileNameOf, folderOf } from './path_display.js';

export const PROJECT_EXTENSION = 'dodstudio';
export const DEFAULT_PROJECT_NAME = `dod_project.${PROJECT_EXTENSION}`;

/** Options for the first save's Save As dialog: `.dodstudio` in the
 *  projects folder. `.json` is a second filter, for a user who wants it. */
export function projectSaveDialogOptions(projectsDir) {
  return {
    title: STRINGS.MAIN.SAVE_PROJECT_SESSION_TITLE,
    defaultPath: projectsDir ? `${projectsDir}\\${DEFAULT_PROJECT_NAME}` : DEFAULT_PROJECT_NAME,
    filters: [
      { name: STRINGS.MAIN.PROJECT_FILTER_NAME, extensions: [PROJECT_EXTENSION] },
      { name: STRINGS.MAIN.JSON_PROJECT_FILTER_NAME, extensions: ['json'] },
    ],
  };
}

/** Options for Save a copy's dialog, the only way to save a project that
 *  needs a newer build to save over it: `<name> copy.dodstudio` beside it. */
export function projectCopyDialogOptions(path) {
  const stem = (fileNameOf(path) || '').replace(/\.(json|dodstudio)$/i, '');
  const name = `${STRINGS.PROJECT_FILE.copyName(stem)}.${PROJECT_EXTENSION}`;
  return {
    ...projectSaveDialogOptions(null),
    title: STRINGS.PROJECT_FILE.SAVE_COPY_DIALOG_TITLE,
    defaultPath: `${folderOf(path)}\\${name}`,
  };
}

/** Load Project's filter: both kinds of project file in one. */
export const PROJECT_OPEN_FILTERS = [
  { name: STRINGS.MAIN.PROJECT_FILTER_NAME, extensions: [PROJECT_EXTENSION, 'json'] },
];

function extensionOf(path) {
  const name = fileNameOf(path) || '';
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
}

/** A file Load Project opens: `.dodstudio`, or an older `.json` project. */
export function isProjectFile(path) {
  const ext = extensionOf(path);
  return ext === PROJECT_EXTENSION || ext === 'json';
}

export function isDemoFile(path) {
  return extensionOf(path) === 'dem';
}
