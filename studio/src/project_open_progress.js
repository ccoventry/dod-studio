// project_open_progress.js
// What the queue's line says while a project opens (#958): "Opening the
// project…" while its file is read and applied, then "Looking for 3 missing
// demos…" while project_demos.js searches the folders for the demos that
// moved (the backend says how many with `missing_demos_search`). Both slide,
// since neither can be measured. The line is handed to the grouping the
// loaded demos start in between (queue_match_groups.js), and the search
// gives it back to the grouping when it ends (queue_progress.js).

import { listen } from '@tauri-apps/api/event';
import { queueProgress } from './queue_progress.js';
import { STRINGS } from './strings.js';

/**
 * @param {object} [deps]  `progress` the queue's line (queue_progress.js),
 *   `listenFn` Tauri's `listen`; both for tests.
 */
export function createProjectOpenProgress({ progress = queueProgress(), listenFn = listen } = {}) {
  let opening = null;
  let searching = null;

  Promise.resolve(listenFn('missing_demos_search', (event) => {
    if (!searching) return;
    progress.show(searching, {
      text: STRINGS.MAIN.lookingForMissingDemos(event?.payload?.missing ?? 0),
      indeterminate: true,
    });
  })).catch((err) => console.error('Failed to register missing_demos_search listener:', err));

  return {
    /** The file is being read. */
    opening() {
      opening = progress.begin({ text: STRINGS.MAIN.OPENING_PROJECT });
    },
    /** The file is applied: the line says `text` (or the line from before),
     *  and the grouping the demos start puts it back when it ends. */
    opened(text = null) {
      progress.end(opening, text);
      opening = null;
    },
    /** The check for demos that moved starts. */
    searching() {
      searching = progress.begin({ text: STRINGS.MAIN.CHECKING_PROJECT_DEMOS });
    },
    /** The search is over (before any dialog about what it found). */
    searched() {
      progress.end(searching);
      searching = null;
    },
  };
}
