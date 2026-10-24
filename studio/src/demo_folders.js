// demo_folders.js
// The demo folder lists the settings file keeps: the pinned folders (shared
// by Capture Studio's scan paths and the Demo Analyzer Explorer's Pinned
// tier, app-wide as in dev), the Explorer's Recent tier, its per-subfolder
// demo-count toggle and its sidebar width. Kept out of main.js, which only
// wires (#683).

import { initAnalyzerPane, refreshAnalyzerQuickLinks } from './analyzer_pane.js';
import { pinnedFoldersOnly } from './project_paths.js';

/** @param {object} app  persist() saves the settings file. */
export function createDemoFolders({ persist }) {
  let pinned = [];
  // Most-recent-first, capped at 10: mirrors dev's `settings.demo_folder_history`
  // (docs/archive/tauri_parity_audit.md Area 3).
  let history = [];
  // Gates the Explorer tree's per-subfolder demo-count badge; dev's
  // `settings.scan_folders_for_demos`, default false.
  let scanFoldersForDemos = false;
  let explorerWidth = 260;

  /**
   * Fills the lists from a loaded settings file. Returns true when the saved
   * pinned list held single demo files (older builds' + Add Demo Files added
   * one path per demo), so the cleaned list is written back once.
   */
  function load(settings) {
    let cleaned = false;
    if (Array.isArray(settings.pinned_folders) && settings.pinned_folders.length > 0) {
      pinned = pinnedFoldersOnly(settings.pinned_folders);
      if (pinned.length !== settings.pinned_folders.length) cleaned = true;
    }
    if (Array.isArray(settings.demo_folder_history) && settings.demo_folder_history.length > 0) {
      history = [...settings.demo_folder_history];
    }
    scanFoldersForDemos = !!settings.scan_folders_for_demos;
    if (settings.analyzer_explorer_width) explorerWidth = settings.analyzer_explorer_width;
    return cleaned;
  }

  // Recent tier: push-front, dedupe, truncate to 10, as dev does.
  async function recordVisit(folder) {
    history = history.filter((f) => f !== folder);
    history.unshift(folder);
    if (history.length > 10) history.length = 10;
    await persist();
  }

  // Pinning or unpinning from the Explorer only saves the path; it doesn't
  // start Capture Studio's highlight scan. A pinned/recent folder that no
  // longer exists is dropped from Recent when clicked, as dev's Quick Links do.
  function startAnalyzerPane() {
    initAnalyzerPane({
      getPinnedFolders: () => pinned,
      pinFolder: async (folder) => {
        if (!pinned.includes(folder)) {
          pinned.push(folder);
          await persist();
        }
      },
      unpinFolder: async (folder) => {
        pinned = pinned.filter((f) => f !== folder);
        await persist();
      },
      getDemoFolderHistory: () => history,
      recordDemoFolderVisit: recordVisit,
      forgetDemoFolderVisit: async (folder) => {
        if (history.includes(folder)) {
          history = history.filter((f) => f !== folder);
          await persist();
        }
      },
      getScanFoldersForDemos: () => scanFoldersForDemos,
      setScanFoldersForDemos: async (enabled) => { scanFoldersForDemos = enabled; await persist(); },
      getAnalyzerExplorerWidth: () => explorerWidth,
      setAnalyzerExplorerWidth: async (px) => { explorerWidth = px; await persist(); },
    });
  }

  return {
    load,
    pinned: () => pinned,
    // Pinned and Recent: + Add Folder fills Recent, not the pins (#533).
    all: () => [...pinned, ...history],
    recordVisit,
    /** Drops `drop` from the pins (the pinned review, #533) and saves. */
    unpin: (drop) => { pinned = pinned.filter((f) => !drop.includes(f)); refreshAnalyzerQuickLinks(); return persist(); },
    /** The fields settings_sync.js writes. */
    settingsState: () => ({ scanPaths: pinned, demoFolderHistory: history, scanFoldersForDemos, analyzerExplorerWidth: explorerWidth }),
    startAnalyzerPane,
  };
}
