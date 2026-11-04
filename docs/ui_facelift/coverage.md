# Redesign coverage against DoD Studio's feature list

**220 items: 86 drawn, 18 cut on purpose, 116 missing** (80 from features.md, 24 from strings.js, 12 from settings_tab.rs). Sources: `docs/features.md`, `studio/src/strings.js` and `goldsrc-hooks/src/settings_tab.rs` on `dev`, against the canvas pages and `simplify-review.md`.

Internal items with no UI (hook internals, file formats, CLI tools, storage paths) are left out. "Drawn" means the page shows the control or the information, not just a related word.

## Missing

| Item | features.md section | Suggested page | How to show it |
|---|---|---|---|
| File menu: New / Load / Save Session (Ctrl+N/O/S) | 2.1 | Pop-up | Project switcher menu: New, Open, Save, with the shortcuts beside them |
| Open Recent and Clear Recent *(source: strings.js)* | 2.1 | Pop-up | The same project switcher lists recent projects, a missing one greyed with Remove |
| Rescan Recent Sessions… *(source: strings.js)* | 2.1 | Pop-up | One line in the project switcher: "Update old projects…" |
| Switch a .json project to .dodstudio (Switch / Keep as .json) | 2.1 | Pop-up | Small prompt after opening: "Switch to the new file type?" Switch / Keep |
| Help → Check for Updates | 2.1 | Settings (this PC) | "Check now" button beside the Updates chips |
| Help → View Logs | 2.1 | Settings (this PC) | "Open activity log" link in the App card |
| Setup checklist in place of the empty queue | 2.1 | Demos | Card at the top: "Finish setting up · 2 of 4" that opens Setup |
| Update <version> available / Restart to update button | 2.1 | Clips | Chip in the top bar on every page: "Update ready · Restart" |
| Unsaved-changes prompt on close | 2.1 | Pop-up | "Save changes to [Project]?" Save / Don't save / Cancel |
| Close while a batch runs prompt | 2.1 | Pop-up | "The game keeps capturing without Studio." Close anyway / Stay |
| Include subfolders | 2.2 | Demos | Switch under + Add demos: "Look in subfolders" (Import takes already draws one) |
| Scan progress line and Cancel Scan (incl. "Looking for demos") | 2.2 | Demos | Thin bar over the list: "Reading 12 of 40" with Cancel |
| Recent and pinned folders, one-time pinned-folders review *(source: strings.js)* | 2.2 | Analyzer | Pinned / Recent in a left column; the one-time review as a pop-up with tick boxes |
| Unreadable demos named with a reason | 2.2 | Demos | Warning chip on the row, like "Crashed last time": "Can't read: cut short" |
| Search box (name, path or map) with × and Esc | 2.2 | Demos | Search box above the demo list |
| Player filter: POV demos one player recorded | 2.2 | Demos | "Recorded by: anyone" chip above the demo list (the drawn "Any player" only filters one demo's highlights) |
| Quick filters: with kills, multi-kill, headshot, POV only | 2.2 | Demos | Chips: "Has a multi-kill" · "Has a headshot" · "First-person only" |
| Tracked only | 2.2 | Demos | Chip: "Has my work" |
| Group by match, player, map, folder, date, demo type | 2.2 | Demos | "Group by" chips; match groups show "Match 3 · h1/h2" headers |
| Sort by (order added, name, newest, oldest, most highlights) *(source: strings.js)* | 2.2 | Demos | Small "Sort" dropdown beside Group by |
| Right-click row menu (preview, review, open in Analyzer, open folder, split, locate, copy name/path, remove) *(source: strings.js)* | 2.2 | Demos | "⋯" button on each demo row with the same items |
| Review: which marks to play (All / Unmarked / Keep / Maybe / Skip) | 2.2 | Demos | Chips beside Review in game: All · Not marked · Keep · Maybe · Skip |
| Review: in-game answer keys (Y M N R B Space S, E form) | 2.2 | Demos | One-line key legend under the button: Y Keep · M Maybe · N Skip · R Replay · Space Next (only Y is drawn) |
| Review: HLTV whose highlights (missing POV, recorders, everyone, picked players/teams) and play order | 2.2 | Demos | For an HLTV demo, chips "Missing their POV · Everyone · Pick…" and "In match order / By player" |
| Review: fast-forward gaps over N s | 2.2 | Demos | Reuse the "Speed up quiet gaps" switch beside Review in game |
| Saved review marks library ("Bring them in / Not now") | 2.2 | Pop-up | "Marks you saved before: 14 highlights." Bring them in / Not now |
| Clear Untracked / Clear Selected / Clear All | 2.2 | Demos | "⋯" menu on the list: "Remove demos with no work", "Remove ticked", "Remove all" |
| Teams list (tags, display name, Same team as, Read Their Teams, Played as) | 2.2 | Demos | Teams strip at the top of Demos: tag chip, name box, "same team as" pick. Simplify-review puts it here, Settings says it has its own page, but neither draws it |
| Missing demo: Locate… / Use found copy | 2.2 | Demos | Row chip "Moved" with Find it / Use the copy found |
| Multi-map demo "2 maps · Split" in the queue | 2.2 | Demos | Row chip "2 maps · Split" linking to Auditor · Split maps |
| Maps needed banner with Download | 2.2 | Demos | Banner: "2 maps missing · Download" |
| No camera data / trimmed copy badges *(source: strings.js)* | 2.2 | Demos | Warning chips on the row: "No camera", "Trimmed copy" |
| Flag chip: Flag defence | 2.2 | Demos | Same chip style: "Flag defence" |
| Status set by hand (✎ and Undo toast) | 2.2 | Clips | Click the status chip to pick one; a small pencil shows it was set by hand |
| Notes per highlight | 2.2 | Clips | Note line under the clip name, also in the variant drawer |
| Typed clip name (wins over the template) *(source: strings.js)* | 2.2 | Clips | Click the clip name to edit it; grey text means automatic |
| Capture history per highlight (Details column) *(source: strings.js)* | 2.2 | Clips | Expand a row: each capture with its take, Open folder, Show video |
| Launch Game | 2.2 | Demos | "Open the game" button beside Review in game |
| View Match Telemetry | 2.2 | Demos | "Stats" link on the demo row that opens the Analyzer |
| Game already running prompts (Force Relaunch / Copy View Command / Cancel; other install or hook build) | 2.2 | Pop-up | "The game is open with a different setup." Restart it / Cancel |
| Disk space needed vs free, and why Start is disabled | 2.2 | Clips | Footer: "Needs 84 GB · 1.2 TB free"; a greyed button says why in one line |
| Steam running / signed-in check | 2.2 | Pop-up | "Steam isn't running." Start Steam / Cancel |
| Game or HLAE error box explained | 2.2 | Pop-up | "The game stopped: <plain reason>." OK / Open log |
| Last batch panel (takes on disk, gaps at 4x) *(source: strings.js)* | 2.2, 2.4 | Clips · capturing | End state of the page: "Last batch · 9 of 9 on disk" |
| OBS drop-rate warning *(source: strings.js)* | 3.1 | Clips · capturing | Yellow line: "OBS is dropping 6% of frames: lower the fps" |
| OBS still recording from a crashed session *(source: strings.js)* | 3.1 | Pop-up | "OBS is still recording." Stop and keep / Leave it |
| 2 GB / 4 GB memory report for hl.exe | 2.4 Paths | Settings (this PC) | Chip on the install row: "4 GB memory" or "2 GB: may crash on big maps" |
| Size the game's menus like the pre-Anniversary engine | 5.1 | Settings (this PC) | Switch in the Game card, only for an Anniversary install: "Normal-size menus" |
| AGR FPS | 2.4 Output | Project settings | Shown under Capture with when AGR is picked: "AGR rate: same as Smoothness" |
| OBS Test Connection button | 2.4 Output | Settings (this PC) | "Test connection" button in the OBS card (only named in a hint today) |
| OBS Capture FPS | 2.4 Output | Project settings | Smoothness shows "OBS: 60" when Capture with is OBS |
| OBS Quality (Small / Standard / High / Max) *(source: strings.js)* | 2.4 Output | Settings (this PC) | Chips in the OBS card with the size per minute |
| Live timeline diagram | 2.4 Timing | Project settings | Small bar under Clip length: 5 s · kills · 5 s |
| Clear Previews… | 2.4 Pipeline | Settings (this PC) | "Clean up old previews…" link in the App card |
| Field of view (first-run mirv_fov 90; r_decals 256 default) | 2.4 Commands | Project settings · Clip look | View row "Field of view": chips 90 · 100 · Custom |
| Import Config… | 2.4 Commands | Project settings · Clip look | "Import a .cfg…" link under Your own commands |
| Profile Save Changes / Rename / Delete *(source: strings.js)* | 2.4 Commands | Project settings · Clip look | "⋯" on your own preset chip: Rename, Delete |
| Scheduled Commands | 2.4 Commands | Clips · new variant | Simplify-review moves them onto the clip or variant, but the drawer has no field: "+ Command at a time" row |
| Console-name autocomplete | 2.4 Commands | Project settings · Clip look | Suggestions list under the Commands box |
| Your game configs' warnings (config.cfg, exec followed) | 2.4 Commands | Project settings · Clip look | Yellow note: "Your config.cfg sets mirv_fov 100" |
| Render Presets | 2.4 Render | Project settings | "Save as preset" chip after Finished clips (Clips shows a "Capture profile" pick but nothing defines it) |
| Render while capturing *(source: strings.js)* | 2.4 Render | Clips | Third item in the Capture menu: "Render each clip as soon as it's captured" |
| Clip-name placeholder chips | 2.4 Render | Project settings | Chip rows under Name clips like, as Rename demos has |
| Crosshair sprite and 4x4 tile pick | 2.4 Players | Pop-up | Click a crosshair cell: grid of tiles to pick from |
| Make your own crosshair… / Add a crosshair file… *(source: strings.js)* | 2.4 Players | Players and sprays | Second button beside Open logos folder |
| Fallbacks for players not listed (POV spray, HLTV spray, crosshair) | 2.4 Players | Players and sprays | Options card: "Everyone else: their own · one picture · none" |
| "Not an exact copy" mark | 2.5 | Auditor · Duplicates | Row chip "Not an exact copy", unticked |
| Pick the kept copy, Reset | 2.5 | Auditor · Duplicates | Click Keep on any row; group shows "You picked this · Reset" |
| Keep copies already in a folder… *(source: strings.js)* | 2.5 | Auditor · Duplicates | Where card row: "Always keep copies in…" |
| Open Recycle Bin, Copy Path, Open Folder *(source: strings.js)* | 2.5 | Auditor · Duplicates | "⋯" on each row; "Open Recycle Bin" after deleting |
| Progress bar and step line | 2.5 | Auditor · Duplicates | Bar under Found: "Checking 120 of 900" |
| Rename chips: Sides ({faction}, {enemy_faction}) and :lower | 2.5 | Auditor · Rename demos | Add a "Sides" chip card and :lower to Tricks |
| Default (reset template) buttons *(source: strings.js)* | 2.5 | Auditor · Rename demos | Small "Default" link beside each template |
| Explorer sidebar (Pinned, Recent, Local, tree, folder counts, Add Pin, Refresh, resizable) | 2.6 | Analyzer | Left column: Pinned / Recent / This PC, with a "Show counts" switch |
| Demos filters (text, type, map, date range, player) | 2.6 | Analyzer | Search box plus chips over the table: Type · Map · Date · Player |
| Multi-map notice with Split now | 2.6 | Analyzer | Banner: "3 maps: report covers dod_flash · Split now" |
| Damaged demo: how far it was read | 2.6 | Analyzer | Banner: "Read up to 14:20: the file is cut" |
| Chat Log filters (team, alive/dead, system) | 2.6 | Analyzer | Chips on the Chat tab: All · Team · System |
| Copy kill-feed command, copyable Steam IDs *(source: strings.js)* | 2.6 | Analyzer | Copy buttons on the Players tab |
| Point at your own Python / upscaler *(source: strings.js)* | 6 | HD textures | "Use my own…" link beside Set up |
| Maps to build (all, or picked maps and patterns) | 6 | HD textures | Build tab: "Maps: all · pick…" with a search box |
| Build options: styles, file types, largest size | 6 | HD textures | Build tab: style chips, type chips, 1024 · 2048 · 4096 |
| Recolour a single piece of floor | 2.8 | Overviews | Hint: "Alt-click to recolour one piece" |
| Hide an area | 2.8 | Overviews | "Hide" in the area's click menu |
| Rename flags | 2.8 | Overviews | Click a flag label to edit it |
| Add labels | 2.8 | Overviews | "+ Label" button (dragging is drawn) |
| BMP format choice | 2.8 | Overviews | Format chips: TGA · BMP |
| Theme, other colour / original colour, Undo, title card, credit, flag icons as seen at, map filter, install pick *(source: strings.js)* | 2.8 | Overviews | Colour chips and Undo over the map; Title, Credit and "Flag icons for" rows in the right column |
| Choose blender.exe / find automatically *(source: strings.js)* | 2.9 | Blender | Browse on the "Needs" row |
| Frames to render (optional) *(source: strings.js)* | 2.9 | Blender | Box on the Render step: "Only frames…" |
| "Batch may still be capturing" wait and Start refusal | 3.5 | Clips | Banner: "The game may still be capturing · waiting for it to close" |
| Scan for Takes | 4 | Clips | Automatic on opening; a small "Look for takes" link for a manual rescan |
| Render row details (codec, FPS, speed, progress, size) | 4 | Clips | Under the status while rendering: "ProRes 422 · 2.1x · 64%" |
| Row actions: Cancel, Retry/Reset, Remove, View Log, Open Folder | 4 | Clips | "⋯" menu on each clip row |
| Tick boxes, Select unrendered, bulk Cancel/Retry/Remove/Delete, Clear Queue | 4 | Clips | Tick boxes plus a bar "3 ticked: Render · Remove" |
| HUD-only row (ProRes 4444 with alpha) | 4 | Clips | Sub-row under the clip like a variant: "HUD layer" |
| Render history per take *(source: strings.js)* | 4 | Clips | Inside the same expanded row as capture history |
| Add Nameplate Script to VEGAS | 4 | Clips | Fourth item in the Export menu |
| Render crash recovery (Recover / Discard) | 4 | Pop-up | "Last render batch stopped early." Recover / Discard |
| Take pictures: middle-frame thumbnail on each row | 4 | Clips | Small picture at the left of each captured row |
| Delete Take | 4 | Clips | "Move take to Recycle Bin" in the row's "⋯" menu |
| In-game DoD Studio window, review overlay and keys, From the start / From here | 5.1, 2.2 | New page | Mock-up of the in-game window: tab names and the review box |
| Offer to rescan demos an older version scanned | 7.3 | Pop-up | "5 demos were read by an older version." Rescan / Later |
| Web analyzer: several demos at once, JSON export | 7.4 | Website · Analyzer | "Open demos…" (several) and an "Export JSON" button |
| Match the player's own view (spec_match_pov) *(source: settings_tab.rs)* | settings_tab: SPECTATING | Project settings · Clip look | Switch in Spectating; Clean clips turns it on |
| Overview icon (hud_overview_team_marker) *(source: settings_tab.rs)* | settings_tab: HUD | Project settings · Clip look | Dropdown beside "Overview map icons": The game's own · HLTV team icon · None |
| No speaker icons on the overview (hud_overview_hide_speaker) *(source: settings_tab.rs)* | settings_tab: HUD | Project settings · Clip look | Switch under the overview rows |
| Stop models flickering (player_fix_model_flicker) *(source: settings_tab.rs)* | settings_tab: PLAYERS | Project settings · Clip look | Switch in Players: "Stop models flickering (HLTV)" |
| Run per-map cfgs (map_run_cfgs) *(source: settings_tab.rs)* | settings_tab: MAP | Project settings · Clip look | Switch in Map: "Run my map .cfg files" |
| HD textures on/off (hd_enabled) *(source: settings_tab.rs)* | settings_tab: HD TEXTURES | Project settings · Clip look | Add "Off" to the HD textures box |
| Seeking skips what it jumps over (demo_seek_skip_between) *(source: settings_tab.rs)* | settings_tab: DEMO PLAYBACK | Project settings · Clip look | Page says playback settings stay out; if kept, an "Advanced" fold |
| Open demos on other drives (demo_other_drives) *(source: settings_tab.rs)* | settings_tab: DEMO PLAYBACK | Project settings · Clip look | Studio sets it per batch; leave out and say so in the page note |
| Show how far a demo has loaded (demo_show_load_progress) *(source: settings_tab.rs)* | settings_tab: DEMO PLAYBACK | Project settings · Clip look | "Advanced" fold switch |
| Fast loading (demo_fast_load) *(source: settings_tab.rs)* | settings_tab: DEMO PLAYBACK | Project settings · Clip look | "Advanced" fold: Off · 8 ms · more |
| ex_interp up to (demo_ex_interp_max) *(source: settings_tab.rs)* | settings_tab: DEMO PLAYBACK | Project settings · Clip look | View row: "Smoother far players" switch |
| DoD Studio window rows (viewdemo opens window, console key, resizable, remember layout, folder lists, counts, hide empty, count subfolders, review keys, review info) *(source: settings_tab.rs)* | settings_tab: DOD STUDIO WINDOW | New page | Page note excludes window settings; show them on the in-game window mock-up, review keys and review info as two switches beside Review in game |

## Contradictions

Places where the canvas says something the app (features.md, strings.js or settings_tab.rs) does not.

1. **Capture mode default** (Project settings): "Video" is the picked chip. The app's default is **Frame sequence** (features.md 3.1). simplify-review.md doesn't change the default.
2. **"Free up space by itself… go to the Recycle Bin"** (Settings (this PC)): "Only when space is needed" works only with permanent delete, because the Recycle Bin frees nothing until it's emptied (features.md 4). This wording comes from simplify-review.md, so as drawn the switch would never free any space.
3. **AGR as an add-on** ("Also record AGR for Blender: saves the scene and camera next to the video" on Clips · new variant; "Also record AGR" on Clips · capturing; the Blender aside): AGR is a capture mode of its own. It records no video, because the picture goes to FFmpeg's null muxer (3.1), so one capture can't also produce the video. The same drawer also offers "AGR for Blender" as a Capture mode.
4. **"Failed: game closed" status** (Clips): highlight statuses are None / Pending / Captured / Rendered. A clip from a crashed batch has no take, and Studio asks "Capture the rest?" (3.5). Only render rows have a Failed outcome.
5. **"Nameplates: HLTV demos only"** (Players and sprays; variant drawer "Nameplates and health bars"): a nameplate is a PNG per player that the VEGAS script puts above that player's clips, in any demo (2.4 Players). The HLTV-only thing is the in-game "Over players" name and health bar (`spec_show_player_info`). The drawer mixes up the two.
6. **Players saved with the project** (Players and sprays aside, "This project / Everyone I have set" tabs): today the players table is a global setting in `settings.json`, not part of the project. This depends on #1085, which is still open.
7. **Spray size 96 px** (Players and sprays): the default is 64. simplify-review.md also says spray size "becomes automatic", yet the page still shows it.
8. **Clip name template** (Project settings) `{player}_{kills}k_{weapon}_{map}` with the example `nifty_5k_mp40_dod_flash`. There is no `{weapon}` (the names are `{weapons}` or `{first_weapon}`). `{map}` drops the `dod_` prefix, so the name would end `_flash`. `{player}` works but is the raw name with the tag; `{name}` is the documented one (strings.js `CLIP_NAME.DESCRIPTIONS`).
9. **Rename preview** (Auditor · Rename demos): `{map}` without `dod_` would give `nifty_5k_v_xyz_avalanche`, not `…_dod_avalanche`. `scrim.dem → …_v_mix_…` uses "mix", but the template has no `|mix`, so the real result is `unknown` (2.5). The `{server}` chip isn't a placeholder in strings.js. The chip cards leave out the Sides row and `:lower`.
10. **Clean clips preset** (Project settings · Clip look): the built-in Clean clips turns Match POV on; hides the scoreboard, voice menus, HLTV messages, spectator bars and warm-up text; shows the scope; and leaves **every other setting at the game's own** (2.4 Commands). The canvas preset also hides the teammate name, the hint box, centre text and plugin text, mutes victory music and restores gunshots, and has no Match POV row at all. The presets "Like the stock game" and "Cinematic" don't exist: Clean clips is the only built-in.
11. **Clip look values versus settings_tab.rs**: "Your own blood: Show when hit" isn't a choice (the choices are The game's own / A little / Most / Nearly all / All). X-ray is drawn as an on/off switch, but the real setting is Off / Through walls / And outlines.
12. **HD style per project** (Clip look "The texture style this project uses"): today the style is set with lines you copy from the HD page into `movie.cfg` (6), not per project. This is a behaviour change the canvas doesn't mark as one.
13. **HD "What is built" table** shows only World / Models / Sprites. There are six types: world, models, sprites, decals, detail and sky (6).
14. **Setup "Four steps"** leaves out the demos step. Setup covers the game, HLAE, FFmpeg, output folders **and demos** (2.1; strings.js `SETUP.MENU_ITEM_TITLE`).
15. **Duplicates**: a copy with the "backed up in pack.7z" chip is ticked for deletion, while the Found card says backed-up demos are "Never offered for deletion". In the app, a backup never *causes* an offer to delete (2.5), but a loose duplicate can still be offered. The card overstates it.
16. **Overviews (desktop)**: "Works in your normal game too" sits next to "Also save 4096×3072", but the high-quality copy only shows with the hook DLL (2.8). The website page gets this right.
17. **Website home**: "the tools are built once and run in both". The web analyzer actually uses a hand-copied renderer (#238, 7.4), and the web build today is only the Analyzer (with JSON export).
18. **Settings (this PC)**: "Players and Teams live on their own pages". No Teams page is drawn, and the nav has no Teams entry.
19. **Drawn but not in the app today** (proposals, so not wrong as such, but not backed by features.md or strings.js): Map lighting page (What's New lists it as #1310, which is after this features.md), Auditor · Backups tab, Pop-up · Import takes, Pop-up · Add your own moment (free camera, campaths, Cinema 4D), "Stop after this clip", "Copy link to stats", "Changes in the game save here", and the website Auditor / Overviews / Studio preview pages (the website Auditor's "_duplicates folder" too).

## features.md out of date

Where strings.js (the app's real labels) has more than features.md says, or says something different. strings.js wins.

- **Highlight Details columns**: strings.js also has **Clip Name** (a typed name wins over the template), **Row #** and **Details** (capture history: each take, Open Folder, Show video). features.md 2.2 lists none of the three.
- **Render while capturing**: a setting exists (each clip renders as soon as it's recorded; with permanent delete, the batch waits for room). features.md doesn't mention it.
- **OBS Quality**: Small / Standard / High / Max (NVENC) on Output Format. Not in features.md 2.4.
- **Master Demo Queue**: **Sort by** (order added, name, newest, oldest, most highlights), a **right-click row menu** (#1111) and **Use found copy** for a missing demo. Not in features.md.
- **File menu**: also **Open Recent / Clear Recent** (#935) and **Rescan Recent Sessions…** (#938). features.md 2.1 lists only New / Load / Save.
- **Teams**: has a **Played as** column.
- **Duplicates**: also **Keep copies already in a folder…**, **Open Recycle Bin**, and per-row Copy Path / Open Folder.
- **Demo Analyzer**: Min/Max **date** filters, **Add Pin**, tree **Refresh**, **Copy kill-feed command** on Player Details. Summary also shows map checksum, the protocols and the most entities in one snapshot.
- **Overviews**: Theme, Other colour / Original colour, Undo, a title card, Credit, "Flag icons as seen at", a map filter, a Half-Life install pick, Start over. None of these are in features.md 2.8.
- **Players**: a **Use as** (in-game name) column, which `{name}` / `{full_name}` use, plus **Make your own crosshair…** and **Add a crosshair file…**.
- **HD Textures**: Download what's missing, Choose python.exe / upscaler folder, and map picking by lines and patterns with Undo changes.
- **Blender**: Choose blender.exe / Find it automatically, and an optional frames box.
- **Warnings features.md doesn't list**: a "no camera" badge and prompt, a "trimmed copy" (entity limit) badge, a crash-map prompt before a batch, an OBS drop-rate line, an "OBS is still recording" prompt, and the **Last batch** panel (features.md mentions it only in passing in Timing).
- **Placeholders**: strings.js has a Highlight group and `{weapons}`, `{first_weapon}`, `{victims}`, `{first_victim}`, `{row}`, `{time}`, `{demo}`, `{half}`, `{match}`, `{team1}`/`{team2}` and `{demo_type}`. `{map}` is the map **without `dod_`**, and Rename Demos' `{kills}` counts the whole demo, not the kill range.
- **Drag-and-drop**: features.md 2.1 says "No drag-and-drop anywhere in the app", but 2.2 (#530) and strings.js (`DROP` toasts) show that dropping demos and folders from Windows works. 2.1 is stale.
- **Queue search**: the placeholder reads "Search filename or map…". features.md says it searches name, path or map.
- **Within strings.js itself**: the Clear Untracked tooltip says "no status, notes, or edited kill range", but its own tracked-work reasons (and features.md) also count review marks and typed clip names. The clip-name tooltip points to "Configuration > Render Output", but the tab is labelled "Render Settings".


<details>
<summary>Full table (all items)</summary>

| # | Item | Section | Status | Page / reason / suggestion | Source |
|---|---|---|---|---|---|
| 1 | File menu: New / Load / Save Session (Ctrl+N/O/S) | 2.1 | Missing | → Pop-up: Project switcher menu: New, Open, Save, with the shortcuts beside them | features.md |
| 2 | Open Recent and Clear Recent | 2.1 | Missing | → Pop-up: The same project switcher lists recent projects, a missing one greyed with Remove | strings.js |
| 3 | Rescan Recent Sessions… | 2.1 | Missing | → Pop-up: One line in the project switcher: "Update old projects…" | strings.js |
| 4 | Switch a .json project to .dodstudio (Switch / Keep as .json) | 2.1 | Missing | → Pop-up: Small prompt after opening: "Switch to the new file type?" Switch / Keep | features.md |
| 5 | .dem Open With and "Open in" target | 2.1 | Drawn | Settings (this PC) | features.md |
| 6 | Help → Check for Updates | 2.1 | Missing | → Settings (this PC): "Check now" button beside the Updates chips | features.md |
| 7 | What's New window | 2.1 | Drawn | Pop-up · What's New | features.md |
| 8 | Help → View Logs | 2.1 | Missing | → Settings (this PC): "Open activity log" link in the App card | features.md |
| 9 | Setup window (Help → Setup…) | 2.1 | Drawn | Setup (first launch) | features.md |
| 10 | Setup checklist in place of the empty queue | 2.1 | Missing | → Demos: Card at the top: "Finish setting up · 2 of 4" that opens Setup | features.md |
| 11 | Make a movie copy (name, also copy demos) | 2.1 | Drawn | Setup (first launch) | features.md |
| 12 | Update <version> available / Restart to update button | 2.1 | Missing | → Clips: Chip in the top bar on every page: "Update ready · Restart" | features.md |
| 13 | Update channel Stable / Experimental | 2.1 | Drawn | Settings (this PC) | features.md |
| 14 | Unsaved-changes prompt on close | 2.1 | Missing | → Pop-up: "Save changes to [Project]?" Save / Don't save / Cancel | features.md |
| 15 | Close while a batch runs prompt | 2.1 | Missing | → Pop-up: "The game keeps capturing without Studio." Close anyway / Stay | features.md |
| 16 | Six notification switches | 2.1, 2.4 | Cut on purpose | "One switch: Tell me when a batch finishes or fails" (drawn on Settings (this PC)) | features.md |
| 17 | Add Demo Files / Add Folder | 2.2 | Drawn | Demos (+ Add demos) | features.md |
| 18 | Drag demos and folders from Windows | 2.2 | Drawn | Demos ("Drop demos or folders") | features.md |
| 19 | Include subfolders | 2.2 | Missing | → Demos: Switch under + Add demos: "Look in subfolders" (Import takes already draws one) | features.md |
| 20 | Scan progress line and Cancel Scan (incl. "Looking for demos") | 2.2 | Missing | → Demos: Thin bar over the list: "Reading 12 of 40" with Cancel | features.md |
| 21 | Recent and pinned folders, one-time pinned-folders review | 2.2 | Missing | → Analyzer: Pinned / Recent in a left column; the one-time review as a pop-up with tick boxes | strings.js |
| 22 | Unreadable demos named with a reason | 2.2 | Missing | → Demos: Warning chip on the row, like "Crashed last time": "Can't read: cut short" | features.md |
| 23 | Demo rows with counts (highlights, in project) | 2.2 | Drawn | Demos (simplified to "N highlights · N in project") | features.md |
| 24 | Search box (name, path or map) with × and Esc | 2.2 | Missing | → Demos: Search box above the demo list | features.md |
| 25 | Player filter: POV demos one player recorded | 2.2 | Missing | → Demos: "Recorded by: anyone" chip above the demo list (the drawn "Any player" only filters one demo's highlights) | features.md |
| 26 | Quick filters: with kills, multi-kill, headshot, POV only | 2.2 | Missing | → Demos: Chips: "Has a multi-kill" · "Has a headshot" · "First-person only" | features.md |
| 27 | Tracked only | 2.2 | Missing | → Demos: Chip: "Has my work" | features.md |
| 28 | Group by match, player, map, folder, date, demo type | 2.2 | Missing | → Demos: "Group by" chips; match groups show "Match 3 · h1/h2" headers | features.md |
| 29 | Sort by (order added, name, newest, oldest, most highlights) | 2.2 | Missing | → Demos: Small "Sort" dropdown beside Group by | strings.js |
| 30 | Right-click row menu (preview, review, open in Analyzer, open folder, split, locate, copy name/path, remove) | 2.2 | Missing | → Demos: "⋯" button on each demo row with the same items | strings.js |
| 31 | Review highlights | 2.2 | Drawn | Demos ("Review in game") | features.md |
| 32 | Review: which marks to play (All / Unmarked / Keep / Maybe / Skip) | 2.2 | Missing | → Demos: Chips beside Review in game: All · Not marked · Keep · Maybe · Skip | features.md |
| 33 | Review: in-game answer keys (Y M N R B Space S, E form) | 2.2 | Missing | → Demos: One-line key legend under the button: Y Keep · M Maybe · N Skip · R Replay · Space Next (only Y is drawn) | features.md |
| 34 | Review: HLTV whose highlights (missing POV, recorders, everyone, picked players/teams) and play order | 2.2 | Missing | → Demos: For an HLTV demo, chips "Missing their POV · Everyone · Pick…" and "In match order / By player" | features.md |
| 35 | Tick Yes for capture | 2.2 | Cut on purpose | "Keep = in the project" (drawn as "Y Keep adds it to the project") | features.md |
| 36 | Review: fast-forward gaps over N s | 2.2 | Missing | → Demos: Reuse the "Speed up quiet gaps" switch beside Review in game | features.md |
| 37 | Saved review marks library ("Bring them in / Not now") | 2.2 | Missing | → Pop-up: "Marks you saved before: 14 highlights." Bring them in / Not now | features.md |
| 38 | Clear Untracked / Clear Selected / Clear All | 2.2 | Missing | → Demos: "⋯" menu on the list: "Remove demos with no work", "Remove ticked", "Remove all" | features.md |
| 39 | Teams list (tags, display name, Same team as, Read Their Teams, Played as) | 2.2 | Missing | → Demos: Teams strip at the top of Demos: tag chip, name box, "same team as" pick. Simplify-review puts it here, Settings says it has its own page, but neither draws it | features.md |
| 40 | Missing demo: Locate… / Use found copy | 2.2 | Missing | → Demos: Row chip "Moved" with Find it / Use the copy found | features.md |
| 41 | Multi-map demo "2 maps · Split" in the queue | 2.2 | Missing | → Demos: Row chip "2 maps · Split" linking to Auditor · Split maps | features.md |
| 42 | Maps needed banner with Download | 2.2 | Missing | → Demos: Banner: "2 maps missing · Download" | features.md |
| 43 | No camera data / trimmed copy badges | 2.2 | Missing | → Demos: Warning chips on the row: "No camera", "Trimmed copy" | strings.js |
| 44 | Crash-map warning | 2.2 | Drawn | Demos ("Crashed last time" chip) | strings.js |
| 45 | Highlight rows: kills, time, duration, weapons | 2.2 | Drawn | Demos | features.md |
| 46 | HLTV Player box | 2.2 | Drawn | Demos ("Any player") | features.md |
| 47 | Kill range (narrowable) | 2.2 | Drawn | Clips · new variant ("Click a kill to start or end there") | features.md |
| 48 | Flag chips: Capping, Cleared the flag | 2.2 | Drawn | Demos | features.md |
| 49 | Flag chip: Flag defence | 2.2 | Missing | → Demos: Same chip style: "Flag defence" | features.md |
| 50 | HS headshot count | 2.2 | Drawn | Demos ("2 HS" chip) | features.md |
| 51 | Review mark Keep / Maybe / Skip | 2.2 | Drawn | Demos | features.md |
| 52 | Status None / Pending / Captured / Rendered | 2.2 | Drawn | Clips | features.md |
| 53 | Status set by hand (✎ and Undo toast) | 2.2 | Missing | → Clips: Click the status chip to pick one; a small pencil shows it was set by hand | features.md |
| 54 | Notes per highlight | 2.2 | Missing | → Clips: Note line under the clip name, also in the variant drawer | features.md |
| 55 | Typed clip name (wins over the template) | 2.2 | Missing | → Clips: Click the clip name to edit it; grey text means automatic | strings.js |
| 56 | Weapon timeline | 2.2 | Drawn | Clips, Demos (weapons text) | features.md |
| 57 | Capture history per highlight (Details column) | 2.2 | Missing | → Clips: Expand a row: each capture with its take, Open folder, Show video | strings.js |
| 58 | Launch Preview | 2.2 | Drawn | Demos ("Watch in game") | features.md |
| 59 | Launch Game | 2.2 | Missing | → Demos: "Open the game" button beside Review in game | features.md |
| 60 | View Match Telemetry | 2.2 | Missing | → Demos: "Stats" link on the demo row that opens the Analyzer | features.md |
| 61 | Game already running prompts (Force Relaunch / Copy View Command / Cancel; other install or hook build) | 2.2 | Missing | → Pop-up: "The game is open with a different setup." Restart it / Cancel | features.md |
| 62 | Capture summary strip | 2.2 | Drawn | Clips (reduced to profile, folder and free space) | features.md |
| 63 | Start Capture Batch | 2.2 | Drawn | Clips ("Capture N" / "Capture and render N") | features.md |
| 64 | Disk space needed vs free, and why Start is disabled | 2.2 | Missing | → Clips: Footer: "Needs 84 GB · 1.2 TB free"; a greyed button says why in one line | features.md |
| 65 | Steam running / signed-in check | 2.2 | Missing | → Pop-up: "Steam isn't running." Start Steam / Cancel | features.md |
| 66 | Game or HLAE error box explained | 2.2 | Missing | → Pop-up: "The game stopped: <plain reason>." OK / Open log | features.md |
| 67 | Batch progress | 2.2 | Drawn | Clips · capturing | features.md |
| 68 | Last batch panel (takes on disk, gaps at 4x) | 2.2, 2.4 | Missing | → Clips · capturing: End state of the page: "Last batch · 9 of 9 on disk" | strings.js |
| 69 | OBS drop-rate warning | 3.1 | Missing | → Clips · capturing: Yellow line: "OBS is dropping 6% of frames: lower the fps" | strings.js |
| 70 | OBS still recording from a crashed session | 3.1 | Missing | → Pop-up: "OBS is still recording." Stop and keep / Leave it | strings.js |
| 71 | Half-Life install list with engine build | 2.4 Paths | Drawn | Settings (this PC) ("Movie copy of Day of Defeat") | features.md |
| 72 | Hide / Show hidden installs | 2.4 Paths | Cut on purpose | "Keep one install picker" | features.md |
| 73 | Game Folder | 2.4 Paths | Cut on purpose | "The Game Folder comes from the movie copy Setup makes" | features.md |
| 74 | HLAE executable | 2.4 Paths | Drawn | Settings (this PC) | features.md |
| 75 | FFmpeg override | 2.4 Paths | Drawn | Settings (this PC) (FFmpeg row) | features.md |
| 76 | GoldSrc Hooks DLL override | 2.4 Paths | Cut on purpose | "The hook DLL ships with the app" | features.md |
| 77 | Use-a-movie-copy note for Steam's own folder | 2.4 Paths | Drawn | Setup (first launch) ("Why a movie copy?") | features.md |
| 78 | 2 GB / 4 GB memory report for hl.exe | 2.4 Paths | Missing | → Settings (this PC): Chip on the install row: "4 GB memory" or "2 GB: may crash on big maps" | features.md |
| 79 | HLAE FFmpeg row / Point HLAE at FFmpeg | 2.4 Paths | Cut on purpose | "The app uses one copy for both" | features.md |
| 80 | Size the game's menus like the pre-Anniversary engine | 5.1 | Missing | → Settings (this PC): Switch in the Game card, only for an Anniversary install: "Normal-size menus" | features.md |
| 81 | Width and height | 2.4 Output | Drawn | Project settings (Size chips) | features.md |
| 82 | Capture FPS | 2.4 Output | Drawn | Project settings (Smoothness chips) | features.md |
| 83 | Capture Mode | 2.4 Output | Drawn | Project settings ("Capture with") | features.md |
| 84 | AGR FPS | 2.4 Output | Missing | → Project settings: Shown under Capture with when AGR is picked: "AGR rate: same as Smoothness" | features.md |
| 85 | Capture codec (Ut Video / FFV1 / x264 / uncompressed) | 2.4 Output | Cut on purpose | "Always Ut Video for Video mode" | features.md |
| 86 | OBS host, port, password, OBS path | 2.4 Output | Drawn | Settings (this PC) (OBS card) | features.md |
| 87 | Launch OBS | 2.4 Output | Drawn | Settings (this PC) (as "Start OBS when a batch starts") | features.md |
| 88 | OBS Test Connection button | 2.4 Output | Missing | → Settings (this PC): "Test connection" button in the OBS card (only named in a hint today) | features.md |
| 89 | OBS Capture FPS | 2.4 Output | Missing | → Project settings: Smoothness shows "OBS: 60" when Capture with is OBS | features.md |
| 90 | OBS Quality (Small / Standard / High / Max) | 2.4 Output | Missing | → Settings (this PC): Chips in the OBS card with the size per minute | strings.js |
| 91 | Initial Delay | 2.4 Timing | Cut on purpose | "The hook DLL already says when the game is ready" | features.md |
| 92 | Pre-roll and Post-roll | 2.4 Timing | Cut on purpose | "Pre-roll and post-roll become automatic" | features.md |
| 93 | Start Lead / Stop Trail | 2.4 Timing | Drawn | Project settings ("Before the first kill" / "After the last kill") | features.md |
| 94 | Short pre/post-roll warning banner | 2.4 Timing | Cut on purpose | Goes with pre/post-roll: "become automatic" | features.md |
| 95 | Live timeline diagram | 2.4 Timing | Missing | → Project settings: Small bar under Clip length: 5 s · kills · 5 s | features.md |
| 96 | Gaps between kills: Keep / Fast-forward | 2.4 Timing | Drawn | Project settings, Clips · new variant | features.md |
| 97 | Flush Decals Between Clips | 2.4 Pipeline | Cut on purpose | "Always on … the right answer for everyone" | features.md |
| 98 | Seek Between Clips | 2.4 Pipeline | Cut on purpose | "Always on … the right answer for everyone" | features.md |
| 99 | Keep the Game Behind Your Windows | 2.4 Pipeline | Cut on purpose | "Always on … the right answer for everyone" | features.md |
| 100 | Save Local Patched Copy, Auto-clear Logs / Previews / Temp Demos | 2.4 Pipeline | Cut on purpose | "They're debugging tools" | features.md |
| 101 | Clear Previews… | 2.4 Pipeline | Missing | → Settings (this PC): "Clean up old previews…" link in the App card | features.md |
| 102 | Demo Scan Workers | 2.4 Pipeline | Cut on purpose | "Automatic from the CPU core count" | features.md |
| 103 | Initial Commands | 2.4 Commands | Drawn | Project settings · Clip look ("Your own commands") | features.md |
| 104 | Field of view (first-run mirv_fov 90; r_decals 256 default) | 2.4 Commands | Missing | → Project settings · Clip look: View row "Field of view": chips 90 · 100 · Custom | features.md |
| 105 | Import Config… | 2.4 Commands | Missing | → Project settings · Clip look: "Import a .cfg…" link under Your own commands | features.md |
| 106 | Profiles incl. built-in Clean clips, Duplicate | 2.4 Commands | Drawn | Project settings · Clip look (Preset chips, "+ Save as new") | features.md |
| 107 | Profile Save Changes / Rename / Delete | 2.4 Commands | Missing | → Project settings · Clip look: "⋯" on your own preset chip: Rename, Delete | strings.js |
| 108 | Scheduled Commands | 2.4 Commands | Missing | → Clips · new variant: Simplify-review moves them onto the clip or variant, but the drawer has no field: "+ Command at a time" row | features.md |
| 109 | Console-name autocomplete | 2.4 Commands | Missing | → Project settings · Clip look: Suggestions list under the Commands box | features.md |
| 110 | Refused / too-long command warnings | 2.4 Commands | Drawn | Project settings · Clip look (hint "refused, with the reason") | features.md |
| 111 | Your game configs' warnings (config.cfg, exec followed) | 2.4 Commands | Missing | → Project settings · Clip look: Yellow note: "Your config.cfg sets mirv_fov 100" | features.md |
| 112 | Destinations (capture folders) | 2.4 Destinations | Drawn | Settings (this PC) ("Capture folders") | features.md |
| 113 | After a clip is finished | 2.4 Destinations, 4 | Drawn | Settings (this PC) (as "Free up space by itself") | features.md |
| 114 | Render codec | 2.4 Render | Drawn | Project settings ("Finished clips" chips) | features.md |
| 115 | Custom FFmpeg args | 2.4 Render | Drawn | Project settings (behind "Custom…") | features.md |
| 116 | Source FPS | 2.4 Render | Cut on purpose | "Every take already records its own FPS" | features.md |
| 117 | Max concurrent renders | 2.4 Render | Cut on purpose | "Automatic from the CPU core count" | features.md |
| 118 | Render Presets | 2.4 Render | Missing | → Project settings: "Save as preset" chip after Finished clips (Clips shows a "Capture profile" pick but nothing defines it) | features.md |
| 119 | Export drives | 2.4 Render | Drawn | Settings (this PC) ("Finished clips" folders) | features.md |
| 120 | When a batch finishes → Finish clips automatically, finish codec per mode | 2.4 Render, 4 | Cut on purpose | "The Clips page gets one Capture and render button" | features.md |
| 121 | Render while capturing | 2.4 Render | Missing | → Clips: Third item in the Capture menu: "Render each clip as soon as it's captured" | strings.js |
| 122 | Clip Name Template | 2.4 Render | Drawn | Project settings ("Name clips like") | features.md |
| 123 | Clip-name placeholder chips | 2.4 Render | Missing | → Project settings: Chip rows under Name clips like, as Rename demos has | features.md |
| 124 | Use in the game | 2.4 Players | Drawn | Players and sprays | features.md |
| 125 | Player rows, add by SteamID or name | 2.4 Players | Drawn | Players and sprays | features.md |
| 126 | Spray from logos / From the demo | 2.4 Players | Drawn | Players and sprays | features.md |
| 127 | Use as (in-game name) | 2.4 Players | Drawn | Players and sprays ("Use as" column) | strings.js |
| 128 | Crosshair sprite and 4x4 tile pick | 2.4 Players | Missing | → Pop-up: Click a crosshair cell: grid of tiles to pick from | features.md |
| 129 | Make your own crosshair… / Add a crosshair file… | 2.4 Players | Missing | → Players and sprays: Second button beside Open logos folder | strings.js |
| 130 | Fallbacks for players not listed (POV spray, HLTV spray, crosshair) | 2.4 Players | Missing | → Players and sprays: Options card: "Everyone else: their own · one picture · none" | features.md |
| 131 | Spray size | 2.4 Players | Drawn | Players and sprays ("96 px") | features.md |
| 132 | Nameplate PNG per player | 2.4 Players | Drawn | Players and sprays (Nameplate column) | features.md |
| 133 | Folder picker | 2.5 | Drawn | Auditor · Duplicates | features.md |
| 134 | Duplicates: Add Folder / All drives | 2.5 | Drawn | Auditor · Duplicates ("Also look in") | features.md |
| 135 | Move to Recycle Bin | 2.5 | Drawn | Auditor · Duplicates | features.md |
| 136 | "Not an exact copy" mark | 2.5 | Missing | → Auditor · Duplicates: Row chip "Not an exact copy", unticked | features.md |
| 137 | Pick the kept copy, Reset | 2.5 | Missing | → Auditor · Duplicates: Click Keep on any row; group shows "You picked this · Reset" | features.md |
| 138 | Keep copies already in a folder… | 2.5 | Missing | → Auditor · Duplicates: Where card row: "Always keep copies in…" | strings.js |
| 139 | Open Recycle Bin, Copy Path, Open Folder | 2.5 | Missing | → Auditor · Duplicates: "⋯" on each row; "Open Recycle Bin" after deleting | strings.js |
| 140 | Progress bar and step line | 2.5 | Missing | → Auditor · Duplicates: Bar under Found: "Checking 120 of 900" | features.md |
| 141 | Archives switch (.zip .7z .rar) | 2.5 | Drawn | Auditor · Duplicates | features.md |
| 142 | "Backed up in pack.7z" chip, backed-up count | 2.5 | Drawn | Auditor · Duplicates | features.md |
| 143 | Split Maps (list, under-a-minute unticked) | 2.5 | Drawn | Auditor · Split maps | features.md |
| 144 | Rename Demos: POV and HLTV templates | 2.5 | Drawn | Auditor · Rename demos | features.md |
| 145 | Rename Demos: placeholder chips | 2.5 | Drawn | Auditor · Rename demos | features.md |
| 146 | Rename chips: Sides ({faction}, {enemy_faction}) and :lower | 2.5 | Missing | → Auditor · Rename demos: Add a "Sides" chip card and :lower to Tricks | features.md |
| 147 | Lowercase the whole name | 2.5 | Drawn | Auditor · Rename demos | features.md |
| 148 | Rename preview with clash _2 | 2.5 | Drawn | Auditor · Rename demos | features.md |
| 149 | Undo Last Rename | 2.5 | Drawn | Auditor · Rename demos | features.md |
| 150 | Default (reset template) buttons | 2.5 | Missing | → Auditor · Rename demos: Small "Default" link beside each template | strings.js |
| 151 | Explorer sidebar (Pinned, Recent, Local, tree, folder counts, Add Pin, Refresh, resizable) | 2.6 | Missing | → Analyzer: Left column: Pinned / Recent / This PC, with a "Show counts" switch | features.md |
| 152 | Demos table | 2.6 | Drawn | Analyzer | features.md |
| 153 | Demos filters (text, type, map, date range, player) | 2.6 | Missing | → Analyzer: Search box plus chips over the table: Type · Map · Date · Player | features.md |
| 154 | Cache all | 2.6 | Drawn | Analyzer ("Cache this folder") | features.md |
| 155 | Multi-map notice with Split now | 2.6 | Missing | → Analyzer: Banner: "3 maps: report covers dod_flash · Split now" | features.md |
| 156 | Damaged demo: how far it was read | 2.6 | Missing | → Analyzer: Banner: "Read up to 14:20: the file is cut" | features.md |
| 157 | Report tabs: Summary, Scoreboard, Player Details, Team Details, Timeline, Rounds, Flags, Chat Log, Kill Map | 2.6 | Drawn | Analyzer (tab labels; only Scoreboard drawn) | features.md |
| 158 | Scoreboard badges (POV, reconnected) | 2.6 | Drawn | Analyzer | features.md |
| 159 | Chat Log filters (team, alive/dead, system) | 2.6 | Missing | → Analyzer: Chips on the Chat tab: All · Team · System | features.md |
| 160 | Copy kill-feed command, copyable Steam IDs | 2.6 | Missing | → Analyzer: Copy buttons on the Players tab | strings.js |
| 161 | Kill Map with engagement-distance table | 2.6 | Drawn | Website · Analyzer (desktop has the tab only) | features.md |
| 162 | What's built table | 6 | Drawn | HD textures | features.md |
| 163 | Use it in the game (style pick) | 6 | Drawn | Project settings · Clip look (HD textures box) | features.md |
| 164 | Map coverage warning | 6 | Drawn | HD textures ("Maps in this project") | features.md |
| 165 | Compare styles | 6 | Drawn | HD textures | features.md |
| 166 | Tools: find or download upscaler and Python | 6 | Drawn | HD textures ("Upscaler and Python: Set up") | features.md |
| 167 | Point at your own Python / upscaler | 6 | Missing | → HD textures: "Use my own…" link beside Set up | strings.js |
| 168 | Maps to build (all, or picked maps and patterns) | 6 | Missing | → HD textures: Build tab: "Maps: all · pick…" with a search box | features.md |
| 169 | Build options: styles, file types, largest size | 6 | Missing | → HD textures: Build tab: style chips, type chips, 1024 · 2048 · 4096 | features.md |
| 170 | Your own styles | 6 | Drawn | HD textures ("My styles" tab) | features.md |
| 171 | Textures that kept their original | 6 | Drawn | HD textures ("Missed textures" tab) | features.md |
| 172 | gl_max_size warning | 6 | Drawn | HD textures | features.md |
| 173 | Recolour an area | 2.8 | Drawn | Overviews | features.md |
| 174 | Recolour a single piece of floor | 2.8 | Missing | → Overviews: Hint: "Alt-click to recolour one piece" | features.md |
| 175 | Hide an area | 2.8 | Missing | → Overviews: "Hide" in the area's click menu | features.md |
| 176 | Rename flags | 2.8 | Missing | → Overviews: Click a flag label to edit it | features.md |
| 177 | Add labels | 2.8 | Missing | → Overviews: "+ Label" button (dragging is drawn) | features.md |
| 178 | Flags / capture zones / spawns / stairs / water switches | 2.8 | Drawn | Overviews | features.md |
| 179 | Save overview | 2.8 | Drawn | Overviews | features.md |
| 180 | BMP format choice | 2.8 | Missing | → Overviews: Format chips: TGA · BMP | features.md |
| 181 | Save folder and backup of your own overview | 2.8 | Drawn | Overviews | features.md |
| 182 | High-quality copy | 2.8 | Drawn | Overviews | features.md |
| 183 | Theme, other colour / original colour, Undo, title card, credit, flag icons as seen at, map filter, install pick | 2.8 | Missing | → Overviews: Colour chips and Undo over the map; Title, Credit and "Flag icons for" rows in the right column | strings.js |
| 184 | Pick .agr, models folder, map, textures | 2.9 | Drawn | Blender | features.md |
| 185 | Import / Build scene / Render / Encode MP4 | 2.9 | Drawn | Blender | features.md |
| 186 | Choose blender.exe / find automatically | 2.9 | Missing | → Blender: Browse on the "Needs" row | strings.js |
| 187 | Frames to render (optional) | 2.9 | Missing | → Blender: Box on the Render step: "Only frames…" | strings.js |
| 188 | Capture the rest after a closed or crashed batch | 3.5 | Drawn | Clips ("Failed: game closed · Capture again") | features.md |
| 189 | "Batch may still be capturing" wait and Start refusal | 3.5 | Missing | → Clips: Banner: "The game may still be capturing · waiting for it to close" | features.md |
| 190 | Scan for Takes | 4 | Missing | → Clips: Automatic on opening; a small "Look for takes" link for a manual rescan | features.md |
| 191 | Render row details (codec, FPS, speed, progress, size) | 4 | Missing | → Clips: Under the status while rendering: "ProRes 422 · 2.1x · 64%" | features.md |
| 192 | Row actions: Cancel, Retry/Reset, Remove, View Log, Open Folder | 4 | Missing | → Clips: "⋯" menu on each clip row | features.md |
| 193 | Tick boxes, Select unrendered, bulk Cancel/Retry/Remove/Delete, Clear Queue | 4 | Missing | → Clips: Tick boxes plus a bar "3 ticked: Render · Remove" | features.md |
| 194 | HUD-only row (ProRes 4444 with alpha) | 4 | Missing | → Clips: Sub-row under the clip like a variant: "HUD layer" | features.md |
| 195 | Render history per take | 4 | Missing | → Clips: Inside the same expanded row as capture history | strings.js |
| 196 | Export Marker List | 4 | Drawn | Clips (Export menu) | features.md |
| 197 | Export VEGAS EDL | 4 | Drawn | Clips (Export menu) | features.md |
| 198 | Add Nameplate Script to VEGAS | 4 | Missing | → Clips: Fourth item in the Export menu | features.md |
| 199 | Render crash recovery (Recover / Discard) | 4 | Missing | → Pop-up: "Last render batch stopped early." Recover / Discard | features.md |
| 200 | Combine Clips… | 4 | Drawn | Pop-up · Combine clips | features.md |
| 201 | Take pictures: nine-frame sheet | 4 | Drawn | Clips · take check (nine frames) | features.md |
| 202 | Take pictures: middle-frame thumbnail on each row | 4 | Missing | → Clips: Small picture at the left of each captured row | features.md |
| 203 | Take checks chip (OK / ⚠) | 4 | Drawn | Clips, Clips · take check | features.md |
| 204 | Delete Take | 4 | Missing | → Clips: "Move take to Recycle Bin" in the row's "⋯" menu | features.md |
| 205 | In-game DoD Studio window, review overlay and keys, From the start / From here | 5.1, 2.2 | Missing | → New page: Mock-up of the in-game window: tab names and the review box | features.md |
| 206 | Offer to rescan demos an older version scanned | 7.3 | Missing | → Pop-up: "5 demos were read by an older version." Rescan / Later | features.md |
| 207 | Web analyzer | 7.4 | Drawn | Website · Analyzer | features.md |
| 208 | Web analyzer: several demos at once, JSON export | 7.4 | Missing | → Website · Analyzer: "Open demos…" (several) and an "Export JSON" button | features.md |
| 209 | Match the player's own view (spec_match_pov) | settings_tab: SPECTATING | Missing | → Project settings · Clip look: Switch in Spectating; Clean clips turns it on | settings_tab.rs |
| 210 | Overview icon (hud_overview_team_marker) | settings_tab: HUD | Missing | → Project settings · Clip look: Dropdown beside "Overview map icons": The game's own · HLTV team icon · None | settings_tab.rs |
| 211 | No speaker icons on the overview (hud_overview_hide_speaker) | settings_tab: HUD | Missing | → Project settings · Clip look: Switch under the overview rows | settings_tab.rs |
| 212 | Stop models flickering (player_fix_model_flicker) | settings_tab: PLAYERS | Missing | → Project settings · Clip look: Switch in Players: "Stop models flickering (HLTV)" | settings_tab.rs |
| 213 | Run per-map cfgs (map_run_cfgs) | settings_tab: MAP | Missing | → Project settings · Clip look: Switch in Map: "Run my map .cfg files" | settings_tab.rs |
| 214 | HD textures on/off (hd_enabled) | settings_tab: HD TEXTURES | Missing | → Project settings · Clip look: Add "Off" to the HD textures box | settings_tab.rs |
| 215 | Seeking skips what it jumps over (demo_seek_skip_between) | settings_tab: DEMO PLAYBACK | Missing | → Project settings · Clip look: Page says playback settings stay out; if kept, an "Advanced" fold | settings_tab.rs |
| 216 | Open demos on other drives (demo_other_drives) | settings_tab: DEMO PLAYBACK | Missing | → Project settings · Clip look: Studio sets it per batch; leave out and say so in the page note | settings_tab.rs |
| 217 | Show how far a demo has loaded (demo_show_load_progress) | settings_tab: DEMO PLAYBACK | Missing | → Project settings · Clip look: "Advanced" fold switch | settings_tab.rs |
| 218 | Fast loading (demo_fast_load) | settings_tab: DEMO PLAYBACK | Missing | → Project settings · Clip look: "Advanced" fold: Off · 8 ms · more | settings_tab.rs |
| 219 | ex_interp up to (demo_ex_interp_max) | settings_tab: DEMO PLAYBACK | Missing | → Project settings · Clip look: View row: "Smoother far players" switch | settings_tab.rs |
| 220 | DoD Studio window rows (viewdemo opens window, console key, resizable, remember layout, folder lists, counts, hide empty, count subfolders, review keys, review info) | settings_tab: DOD STUDIO WINDOW | Missing | → New page: Page note excludes window settings; show them on the in-game window mock-up, review keys and review info as two switches beside Review in game | settings_tab.rs |

</details>
