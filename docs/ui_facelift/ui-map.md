# DoD Studio UI map (facelift groundwork)

Source: `ccoventry/dod-studio`, `studio/` (read 2026-10-10).

## What the app does
Batch-captures killstreak highlights from Day of Defeat 1.3 demos: scan demos, pick streaks, drive HLAE/hl.exe to record them, then transcode the takes with FFmpeg. Two side tools: a duplicate-demo Auditor and a match-stats Analyzer.

## How the UI is built
- Tauri v2 + Vite, **plain JavaScript, no framework**. One `index.html` (1,051 lines) holds every screen and modal; `src/*.js` modules (~12k lines) mutate the DOM directly.
- Screens are shown and hidden by setting inline `style.display` in `nav.js`, not by routing.
- Styling: one `styles.css` (1,722 lines) with only ~9 CSS variables. 126 inline `style=` attributes in `index.html` plus ~100 more in JS (67 in `analyzer_pane.js`), with ~29 hardcoded hex colours (`#ff5555`, `#888`, `#333`...).
- All copy lives in `strings.js` (good: a redesign can relabel without hunting).

## Screens and flow
Top bar: File / Help menus, then 3 tabs: **Studio**, **Demo Auditor**, **Demo Analyzer**.

Studio has a second tab row: **Capture / Render / Configuration**.
1. **Capture**: left pane = "Directory Scan & Management" (Add Files / Add Folder / Cancel Scan) + "Master Demo Queue" table (8 columns: Highlights, Selected, Pending, Captured, Rendered, Actions; Search, Clear Untracked / Clear Selected / Clear All). Right pane = "Highlight Details" table per demo (9 columns, inline kill-range number inputs, status dropdown, notes) with 5 header controls (Launch Preview, View Match Telemetry, Generate All Previews, Launch Game (HLAE), Min Kills), plus a collapsed "Advanced Diagnostics" canvas. Footer: Start Capture Batch / Cancel, status, disk free/required.
2. **Render**: Scan for Takes, a 10-column job table, Start / Cancel All / Reset All / Remove All (Not Rendering).
3. **Configuration**: one panel with **8 sub-tabs** (Paths, Output Format, Timing, Pipeline, Commands, Destinations, Render Output, Notifications), ~38 inputs, including OBS connection, six timing values, cleanup toggles.

Analyzer: folder explorer sidebar, demo list, report with 7 sub-tabs. Auditor: folder picker, duplicate groups, delete.

## Where the clutter comes from
1. **Three levels of tabs** (top tabs, Studio sub-tabs, 8 config tabs / 7 analyzer tabs) and the main job (capture) is split across Capture, Render and Configuration rather than reading as one flow.
2. **Button walls with equal weight.** Highlight Details header carries 5 controls, the queue header 4, render footer 4; destructive actions (Clear All, Remove All) sit beside routine ones, styled ad hoc in red.
3. **Wide data tables** (8, 9 and 10 columns) that show every status counter and per-row edit control at once.
4. **Setup and expert settings always in reach**: one-time paths (HL, HLAE, FFmpeg, hooks DLL, OBS) share space with per-batch choices; timing exposes Pre-roll, Post-roll, Start Lead, Stop Trail, Initial Delay, FF Speed together.
5. **No real design system**: inline styles and hex colours everywhere mean spacing, colour and emphasis drift screen to screen ("egui-panel" classes are a leftover from the old egui app).

## Starting points
1. **Design tokens first.** Expand `:root` into a full token set (surfaces, text, accent, danger, spacing, radius) and move inline styles into classes. Low risk, no behaviour change, and every later step looks consistent for free.
2. **Make Studio a guided pipeline**: Demos, then Highlights, then Capture, then Render, as one stepper with a single primary action per step. Fold the Master Queue's five status columns into one progress chip; move rarely used actions (Generate All Previews, Launch Game, Clear Untracked/Selected/All, Reset/Remove All) into a per-screen overflow menu.
3. **Split Settings from per-batch options**: move paths, OBS, notifications, destinations and render output into a Settings dialog (or a first-run setup check), keep only resolution/FPS/codec and Min Kills near the Capture button, and hide the six timing knobs behind an "Advanced" disclosure with sensible defaults.
