# UI facelift: design notes

Work in progress for the DoD Studio UI facelift. These are design notes only and no code has changed. The live design is the canvas at https://claude.ai/artifact/8mYZLpBitBu4WNjd5FWVMn. It's private to Chris, so `canvas-pages.md` holds a text copy of every page.

| File | What it is |
|---|---|
| `canvas-pages.md` | The visible text of every canvas page (Studio, Studio tools, Tools, Settings, Clip look, Website, Pop-ups), as of 2026-11-04 |
| `coverage.md` | A check of the canvas against `docs/features.md`, `studio/src/strings.js` and `goldsrc-hooks/src/settings_tab.rs` on dev: what's drawn, what's cut on purpose, what's missing, and contradictions |
| `simplify-review.md` | Settings the redesign cuts or makes automatic, and why |
| `ui-map.md` | Map of the current UI (Tauri v2 + Vite, plain JS, single `index.html`) |

## Status as of 2026-11-04
- Contradictions 1–5, 7–11 and 13–16 in `coverage.md` are fixed on the canvas, and so is 18. Contradictions 6, 12 and 17 are deliberate proposals: players per project (#1085), HD style per project, and more tools on the website (#603). Item 19 lists new ideas that are on the canvas on purpose.
- The 116 "Missing" items haven't been triaged. Some should be added, and many should stay off screen on purpose to keep the app simple.
- The main colour is undecided (the canvas uses neutral off-white).

## Design rules Chris has set
- Few settings and nothing buried in menus. Prefer chips and switches over typed values, with a "Custom…" chip where power users need it.
- Clips must stay uncluttered, but show every clip's status (Pending / Captured / Rendered) at a glance.
- Studio is a project with two tabs: Demos (find and add clips) and Clips (clips and variants, with capture and render run from there). Pressing Keep during an in-game review adds the clip as Pending.
- The sidebar has three groups. Studio: Demos, Clips, Blender. Studio tools (only through the hook DLL while capturing): Players and sprays, Map lighting, HD textures. Tools: Analyzer, Auditor, Overviews.
- The term is "HLAE Game Capture". The game's own `.cfg` files are detected and warned about, never written.

## Review prompt (for an agent working in this repo)
> Read everything in `docs/ui_facelift/`. Then check `coverage.md` against the real code on this branch: `studio/index.html`, `studio/src/*.js`, `studio/src/strings.js`, `studio/src-tauri/src/` and `goldsrc-hooks/src/settings_tab.rs`.
> 1. Find user-facing features, settings or states (errors, empty states, first run, long-running progress, crash recovery) that `coverage.md` misses entirely.
> 2. Mark any "Missing" or "Contradiction" row that is wrong, with the file and line that proves it.
> 3. For each Missing item, recommend Add (and where), Automatic, or Drop, following the design rules in `README.md`.
> 4. List anything the redesign doesn't account for, such as workflows, edge cases, or features in open issues and PRs that will need a place on screen.
>
> Write the results to `docs/ui_facelift/review.md`. Don't change any code or any other file.
