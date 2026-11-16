// strings.js
//
// Single centralized source for every user-facing English string in the
// studio frontend. NOT an i18n system — no language switching, no
// key-based lookup abstraction beyond plain named constants/functions. The
// point is purely "one place to find/edit any UI string."
//
// Grouped by pane/feature area for readability. Static strings are plain
// values; strings built from a template/interpolation are functions that
// return the built string (keeps the interpolation logic here, not
// scattered across the pane files).
//
// Rust backend error strings (src-tauri/) and console.log/console.error
// developer diagnostics are explicitly OUT OF SCOPE and are not represented
// here — only text actually shown to the end user.

import { SHELL_STRINGS } from './strings/shell.js';
import { SHARED_STRINGS } from './strings/shared.js';
import { WORKSPACE_STRINGS } from './strings/workspace.js';
import { HIGHLIGHTS_STRINGS } from './strings/highlights.js';
import { CAPTURE_CONFIG_STRINGS } from './strings/capture_config.js';
import { CAPTURE_STRINGS } from './strings/capture.js';
import { RENDER_STRINGS } from './strings/render.js';
import { REPAIR_STRINGS } from './strings/repair.js';
import { AUDITOR_STRINGS } from './strings/auditor.js';
import { ANALYZER_STRINGS } from './strings/analyzer.js';
import { PAGES_STRINGS } from './strings/pages.js';
import { SETUP_STRINGS } from './strings/setup.js';

// One area per file under strings/, so a feature PR edits its own area's file and
// not one 4,000-line file every PR conflicts in (#604). Key names are unique
// across the areas (strings.test.js).
export const STRINGS = {
  ...SHELL_STRINGS,
  ...SHARED_STRINGS,
  ...WORKSPACE_STRINGS,
  ...HIGHLIGHTS_STRINGS,
  ...CAPTURE_CONFIG_STRINGS,
  ...CAPTURE_STRINGS,
  ...RENDER_STRINGS,
  ...REPAIR_STRINGS,
  ...AUDITOR_STRINGS,
  ...ANALYZER_STRINGS,
  ...PAGES_STRINGS,
  ...SETUP_STRINGS,
};
