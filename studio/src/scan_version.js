// scan_version.js
// Which queued demos an older DoD Studio scanned. Each scan stamps a demo
// with `highlights_version`, the version of highlight detection that listed
// its highlights (`analysis::highlights::HIGHLIGHTS_VERSION`; a Rust test
// checks this copy agrees). A demo without it, or without a field a newer
// scan adds, is out of date: the queue scan parses it again instead of
// skipping it as unchanged, and opening a project offers to rescan it
// (project_rescan.js).

import { demoHasTeams } from './project_teams.js';

export const HIGHLIGHTS_VERSION = 1;

/**
 * True when `demo` was scanned by this version of highlight detection and
 * has every field a scan now fills: file_key, map_name (#441), teams (#445),
 * signon_maps (#217), no_camera_data (#920), and per highlight `spectated`
 * (#247) and `viewdemo_times` (#434).
 */
export function scanIsCurrent(demo) {
  if (!demo) return false;
  return Boolean(demo.file_key)
    && demo.map_name !== undefined
    && demoHasTeams(demo)
    && demo.signon_maps !== undefined
    && demo.no_camera_data !== undefined
    && (demo.highlights_version ?? 0) >= HIGHLIGHTS_VERSION
    && (demo.streaks || []).every((s) => s.spectated !== undefined && s.viewdemo_times !== undefined);
}

/** The demos a rescan would bring up to date; a missing file can't be read. */
export function outdatedDemos(demos) {
  return (demos || []).filter((d) => !d.missing && !scanIsCurrent(d));
}
