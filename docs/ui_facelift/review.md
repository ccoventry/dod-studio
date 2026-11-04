# UI facelift review

Checked on 2026-11-04 against `claude/project-thread-u4stt5`, whose code is the same as `dev` at e429382d. Issues and PRs are as they stood that day. For the design agent and Chris. No code changed, and this is the only file added.

**How it was checked.** All 116 Missing rows and all 19 contradictions were checked against the code, and every row in section 3 carries its proof. A separate high-effort pass then reopened the cited code for every claim in the decisions, the High gaps, sections 2 and 5, and every proof, Drop and Automatic row. Its corrections are in this version. Proofs are `file:line`, relative to the repo root.

**Result.** 100 of the 116 Missing rows hold as written, 12 are partly wrong and 4 are wrong. Recommendations: 94 Add, 8 Automatic, 12 Drop, 1 Move, 1 already drawn. Most Adds are prompts or row details that cost no permanent screen space. Contradictions 10, 11 and 18 are not fully fixed, although the README says they are. Section 1 lists 37 gaps that `coverage.md` misses, and the worst are about losing a capture or capturing nothing. Setup also carries one false safety claim: see the first item under New contradictions.

**For the design agent.** Draw everything here, including the open decisions. For each decision below, draw the recommended version and mark it undecided on the canvas. The pages to add are listed under "Pages and states to draw".

## Needs Chris's call

Each has a recommendation, so none of them blocks the drawing.

| # | Decision | Recommendation | Why it's a real choice |
|---|---|---|---|
| 1 | How to capture part of a project (G10) | Tick boxes on Clips, with a bar "45 ticked · Capture" | Keep = in the project removes today's ticks, and the Capture button takes every Pending clip. WSOD25 has 364 Keeps and is being captured 45 at a time |
| 2 | Where Teams live | "Teams" link on Demos opening a pop-up, with the same pop-up reachable from Project settings | `simplify-review.md` says Demos, Settings says project settings, the Clips page's icons still hold a `teams` entry, and no page draws it. Rename demos depends on it |
| 3 | "Capture profile [Montage 1080p]" on Clips | Remove the name. Keep one line in its place: "1440p · 120 fps · Frames · Clean clips", linking to Project settings | Nothing on the canvas defines it, and keeping it means building #444 on top of #1085. But closing #444 loses a look that's shared across movies and exported to a friend. "Start from: another project" (section 4) covers only the first half |
| 4 | Where Scheduled Commands go | Clip look, second box: "At each clip: command, seconds, before or after" | `simplify-review.md` moves them onto a clip because "a command at tick 1200 is about one clip". That isn't how they work: each one is an offset from every highlight (`studio/index.html:719`, `studio/src/roll_floors.js:36-41`). An override per variant could come later |
| 5 | Analyzer's folder tree (#151) | Replace it with a folder drop-down (Recent, Pinned), plus dragging a folder in | Simpler, but removes the tree, folder counts and Refresh, which a large library may use |
| 6 | Sort by and Launch Game (#29, #59) | Drop both. Newest first becomes fixed, and the game starts when something needs it | Both exist today. Keep Sort if "Most highlights" gets used, and Launch Game if the game is ever opened by hand to try settings |
| 7 | Field of view (#104) | One row at the top of Clip look: 90 · 100 · Custom. It isn't in View, whose heading says first-person only, because `mirv_fov` applies to every demo | Against the few-settings rule, but it's a common look choice, the variant mock-up already shows "FOV 100", and a `mirv_fov` in a config once changed a capture unnoticed |
| 8 | Spray size | Keep it. It's drawn at 64 px on Players and sprays | `simplify-review.md` says it becomes automatic, and the canvas still shows it |
| 9 | Take deleted as soon as its clip renders | Three chips as drawn, with this state gone. "Is deleted when space runs low" covers most needs | The app has four states (`studio/index.html:746-756`). Anyone who wants takes gone at once loses that |
| 10 | Watching a clip record | A "Watch the game while it records" item in the Capture menu | "Always behind your windows" removes the only way to see a bad take happen (`studio/src/strings.js:578`) |

## Pages and states to draw

New pages:
- **Clips · rendering:** the capturing page's twin (G4).
- **Clips · batch finished:** Last batch, with rendered, failed and cut-short takes (#68, G14).
- **Project switcher:** New, Open, Save, recent projects, Project settings (#1, #2, G12).
- **Help menu:** see G11.
- **OBS test:** see G7.
- **No project open and empty states:** see G12.

Pop-ups, one mock-up each:
- Start checks (G1): crash map, no camera data, several maps, demos changed, demo missing.
- Project open (G3): demos moved, demos still missing, rescan offer (#206).
- Closing (#14, #15): unsaved changes, batch running, batch not started.
- Crash recovery: "Capture the rest?" (#188), render recovery (#199), OBS still recording (#70).
- Game and Steam: game already running (#61), Steam not running (#65), game stopped (#66).
- Deleting: permanent-delete confirm and "can't make room" (G8), delete take with size (G21), clear all, and remove demos with work (#30, #38).
- Teams, crosshair tile grid (#128), saved marks (#37), switch to `.dodstudio` (#4).
- New project: start from another project or defaults (#1085). Project made with another install (#1085).
- Make a movie copy: name, also copy demos, size, no room, progress, Stop. It's drawn only as Setup text today (`studio/src/strings.js:3499-3529`).
- A toast with an action button. Undo, Stop and "Use all found copies" are toast actions, and the canvas never draws a toast (`studio/src/toast.js:10-15`).

States on existing pages:
- **Clips:** greyed Capture with a reason line (G2), status and capture history in an expanded row (#57, #195), the tick bar (G10, #193), and a failed render (G15).
- **Demos:** scan bar (#20), filter row (#24-#27), banner for missing maps (#42), row chips (#40, #41, #43), Review's "Stop review" (G16).
- **Settings:** failure states for HLAE and FFmpeg (G6), the Steam-folder warning (G5), capture folder chips (G17).
- **Tools:** HD build and download progress, Cache this folder with Stop, a Blender step running (`studio/index.html:1389,1469,1618-1620,1816-1818`).

## 1. What `coverage.md` misses

Ranked by harm if the redesign forgets it. None of these has a Missing row or a place on the canvas. A few get a passing mention in `coverage.md`, and rows that cover part of a gap are named.

### High: lost captures, wasted batches, or a risk to the account

| Gap | What's in the app | Proof | Rec and where |
|---|---|---|---|
| G1. Prompts that stop or confirm a batch | Crash-map confirm, no-camera confirm, "picked demos have more than one map: Split them now?", "demos changed on disk: Rescan?", picked demo missing. `coverage.md` mentions the crash-map and no-camera prompts only in its "features.md out of date" list, with no row and no place on screen | `studio/src/capture_pane.js:1198-1213`, `studio/src/queue_split.js:110-115`, `studio/src/project_demos.js:142-148` | Add: pop-ups opened by Capture. Each lists the demos and offers the fix (Split them, Rescan, Start anyway) |
| G2. Why Capture is greyed | Greyed: banned commands, blank `hl.exe` or HLAE path, nothing picked, no capture folder, no usable folder, not enough space. Live, with a warning line only: OBS not connected (the click starts OBS), a partly bad folder list, a folder that will be created, short of room when take cleanup or Render while capturing will cover it. Row #64 names only disk space | `studio/src/capture_pane.js:206-405` | Add: Clips footer, one reason line plus tooltip, with a fix link ("Finish setup", "Fix commands") |
| G3. Checks when a project opens | Moved demos ("Use matching copies?"), still-missing demos ("Use all found copies"), "Looking for N missing demos" progress. Row #40 covers only the row chip. Without these, a moved project opens with every demo missing | `studio/src/project_demos.js:20-90`, `studio/src/project_open_progress.js:1-20` | Add: pop-up plus a progress line over the Demos list |
| G4. The rendering stage has no page | "Rendering 3 of 9", batch progress, Cancel, "Done in 4 min: 8 rendered, 1 failed". After Capture and render, the long encode shows nothing. Row #191 only covers text on a row | `studio/src/strings.js:1019-1020,1031-1034`, `studio/src/render_batch_progress.js` | Add: Clips · rendering, built from the capturing page, ending in a summary |
| G5. Studio pointed at Steam's own Half-Life | The warning "don't join a game server from a game DoD Studio started" shows under the install row. The canvas only covers this in first-run Setup (row #77), and Settings can still pick that install | `studio/src/config_wiring.js:81`, `studio/src/strings.js:521` | Add: Settings · Game, amber note whenever that install is picked, with "Make a movie copy" |
| G6. HLAE and FFmpeg failure states | HLAE's hook file missing or quarantined by antivirus, an FFmpeg override that isn't FFmpeg, a stale HLAE FFmpeg link, and Video mode with no FFmpeg HLAE can find, which records no video. The canvas shows only "Found" | `studio/src/config_wiring.js:238-255`, `studio/src/strings.js:512,535-548` | Add: Settings · Game, red status line plus one fix button. See also the cut of #79 in section 5 |
| G7. OBS safeguards | "Test OBS capture" and "Find my highest steady FPS" with a results table, not mentioned anywhere. An amber line when OBS isn't connected; pressing Capture starts OBS and retries | `studio/index.html:499-503`, `studio/src/obs_capture_test.js:39`, `studio/src/strings.js:3605-3653`, `studio/src/capture_pane.js:325-331,1161` | Add: OBS card buttons opening a results pop-up, "Use this" sets Smoothness. Amber line on Clips: "OBS isn't running. Capture will start it." Grey Capture only when no OBS path is set, the one case the retry can't fix (`studio/src/obs_connection_ui.js:127-128`) |
| G8. Deleting takes for good | Turning permanent delete on asks first. A batch that can't make room stops with "No takes were deleted". Both are decisions, and only the setting is drawn | `studio/src/take_cleanup.js:52`, `studio/src/strings.js:1127-1148` | Add: confirm when the chip is picked, pop-up "This batch needs 84 GB; 61 GB can be freed" |
| G9. Render while capturing can pause the batch | "Waiting for renders to free room (about N min)", only when takes are deleted after rendering (`studio/src/render_while_capturing.js:40-41`). `coverage.md:157` mentions the wait with no row, and row #121 has only the switch, so the batch looks stuck | `studio/src/render_while_capturing.js:109-113`, `studio/src/strings.js:1151-1170` | Add: status line on Clips · capturing, and a pop-up if the batch stops early |
| G10. Capturing part of a project | Today every highlight has a tick and a batch takes the ticked rows. The canvas button takes all Pending clips, and Clips has only a status filter, no search or filter by player or demo. Row #193 covers ticks for renders only | `studio/src/detail_pane.js:513`, `studio/src/strings.js:265` | Add: ticks, tick bar, and search plus player and demo filters on Clips (decision 1) |
| G11. No Help menu | The canvas says "Help → Setup…" and "Help → What's New", but no page draws a Help menu, and the File and Help menus are gone from the layout. About, What's New, Setup, View Logs and Check for Updates all live there | `studio/index.html:47-58` | Add: Help item at the foot of the sidebar, holding Setup, What's New and About, then #1014 and #934. Check for Updates and the log move to Settings (rows #6, #8) |
| G12. No project open, and unsaved | The header shows "No session loaded" or "name.dodstudio · unsaved", and a project with nothing in it never prompts. The canvas always shows "[Project name]". Nothing shows Demos or Clips on a first launch, an empty project, or what each Studio page does with no project | `studio/src/project_session.js:100-116`, `studio/src/strings.js:61` | Add: "Untitled project" and a dot when unsaved in the top bar, plus a no-project state. Saved marks (#848) rely on a review working without a project |

### Medium: confusion or a wrong result

| Gap | What's in the app | Proof | Rec and where |
|---|---|---|---|
| G13. Progress after pressing Capture | Checking OBS, disk space, reading demos N of M, maps, paths, game, Steam. Cancel is greyed while takes are being checked | `studio/src/capture_precheck.js:1-60`, `studio/src/capture_pane.js:915,921,1122` | Add: Clips footer, status line and bar from the click until the game starts |
| G14. Cut-short takes | A take cut by Cancel or a closed game counts as not captured, but its folder exists. A video can stop early | `studio/src/batch_results.js:1-12`, `studio/src/strings.js:776-785,2509-2516` | Add: "Cut short" chip on the Pending row and in Last batch |
| G15. Failed render | Failed and Interrupted outcomes, and the FFmpeg error log. Row #192 names View Log, but the canvas has no failed render state | `studio/src/render_pane.js:208`, `studio/index.html:2085-2090` | Add: "Render failed" on the row, Show log and Render again in "⋯" |
| G16. A review is running | Stop is only a toast action, so a dismissed toast leaves only S in the game. Dead ends such as "Nothing to review" are toasts too | `studio/src/review_mode.js:97,120-139`, `studio/src/strings.js:2882-2915` | Add: Review in game becomes "Stop review", and reasons sit under the button |
| G17. Capture folder states | Exists, will be created, drive missing, and two folders on one drive adding no room. The canvas shows only free space | `studio/src/capture_output_status.js:1-12`, `studio/src/config_wiring.js:41-48` | Add: chip on each folder row, amber line for a shared drive |
| G18. Players page warnings | No PNGs in the logos folder, a named spray or crosshair missing, a fallback not chosen, "Use in the game" off, nameplate file gone | `studio/src/players_ui.js:348-359`, `studio/src/nameplate_cell.js:1-8` | Add: tip card, plus "(missing)" on the cell |
| G19. Split maps states | "Already split" chip, Include subfolders, find progress with Cancel, Show in folder | `studio/index.html:1226-1233` | Add: on Auditor · Split maps, alongside #876 |
| G20. Auditor safety | Confirms before Rename and Recycle. A copy in the game folder or the queue isn't ticked, with a reason. The canvas shows only "In the project" on the kept row | `studio/src/demo_rename_ui.js:352`, `studio/src/auditor_pane.js:500`, `studio/src/strings.js:1307,1309` | Add: small pop-ups, and a row chip "In the game folder" |
| G21. Clearing space after a project | Rows #193 and #204 cover the buttons. New here: the measuring and recycling progress | `studio/src/take_delete.js:44-91`, `studio/index.html:1051-1057`, `studio/src/render_pane.js:709-710` | Add: tick bar on Clips, with "Finished, older than a week…" |
| G22. A take vanished from disk | Reported only in a scan summary toast, "N gone (3 were queued)" | `studio/src/render_scan_summary.js`, `studio/src/strings.js:1049` | Add: "Take gone" chip with Capture again |
| G23. HD built by an older version | The status can say a style is stale, and Build repaints only that part | `studio/src/hd_outdated.js:14-30`, `studio/src/hd_pane.js:494` | Add: "Older build" chip on the style row, with Build as the main button |
| G24. Overviews Show options | Breakable floors, steep slopes, spawn protection with its colours, spawn names, flag names | `studio/index.html:1700-1708,1730-1752` | Add: to the Show box, colours behind a "Look…" link |
| G25. Render with no export folder | Start Render Batch stays greyed, and Scan has a "no capture folder" error | `studio/src/strings.js:1041,1044`, `studio/src/render_pane.js:550,930-940` | Add: main button greyed with "Add a folder for finished clips in Settings" |
| G34. Reviewing many demos | Review plays the ticked demos, or every demo shown. The canvas puts it on one demo's header with no ticks on the list, so a review across a whole movie can't start. Row #38's "Remove ticked" also needs those ticks | `studio/index.html:152,230`, `studio/src/strings.js:2837` | Add: tick boxes on the Demos list, and Review in game above the list ("Review 5 demos"). One demo stays in its "⋯" |
| G35. Kill range on a clip | Editable on each highlight, with a reset. The canvas edits it only in the new-variant drawer (row #47), and Clips shows "Kills 2–6 of 6" with no way to set it. Narrowing a clip shouldn't mean making a variant | `studio/src/detail_pane.js:517-522` | Add: in the expanded row, the drawer's kill picker with Reset |

### Low: small states to keep when the page is built

| Gap | Proof | Rec and where |
|---|---|---|
| G26. Switching install while the game runs offers to close it first | `studio/src/install_picker.js:17,186` | Add: pop-up |
| G27. Windows blocking notifications makes the switch do nothing | `studio/src/os_notifications.js:23-29` | Add: switch greyed with the reason |
| G28. Take check states "Checking…", "Couldn't check" and "no plan recorded" | `studio/src/take_checks.js:54`, `studio/src/strings.js:985-1000` | Add: chip text with a tooltip |
| G29. Two overlapping highlights recorded as one take flip to Captured together | `studio/src/detail_pane.js:497-503` | Add: tooltip on the two status chips |
| G30. A Skip highlight can't be picked for capture | `studio/src/detail_pane.js:513`, `studio/src/strings.js:304` | Add: greyed, with that reason |
| G31. Blender's engine and quality picks, Cancel and Refresh are in prose or absent | `studio/index.html:1553,1605-1618` | Add: two chip rows on Render, Stop while a step runs |
| G32. The website Analyzer has 8 tabs and no Kill map, plus a "None of those files are .dem demos" error. Row #208 covers several demos and JSON | `web-analyzer/www/index.html:17,22,33-40`, `web-analyzer/www/strings.js:208-222` | Add: draw the demo list, and mark Kill map and Copy link as planned |
| G36. A movie copy with no language file shows in-game text as codes such as `#Dod_Spec_Allies_Score` (#925) | `studio/src/strings.js:403-411`, `studio/src/game_folder_ui.js:102` | Add: amber line on Settings · Game, with "Show me the file" |
| G37. The update window: check at start, notify when found, notes for the pending update, download bar. Rows #6 and #12 cover only the button and the chip | `studio/index.html:2182-2215` | Add: "Check at start" beside the Updates chips, and a small update pop-up with notes and progress |
| G33. Keep when building: Up and Down on the demo list, Menu or Shift+F10 for the row menu, a drop refused during a scan, the `.dodstudio` double-click, Overviews edits "kept aside" after a map change, the menu-scale switch greyed for a foreign file | `studio/src/master_pane.js:279-300,659-668`, `studio/src/strings.js:3417-3427`, `studio/src/launch_files.js:1-12`, `studio/src/overviews_pane.js:1006-1007`, `studio/src/menu_scale_setting.js:23` | Automatic: behaviour to keep, no new control |

## 2. Rows and contradictions that are wrong

### Missing rows

| # | The row says | What's true | Proof |
|---|---|---|---|
| 78 | Paths reports 2 GB or 4 GB memory | Only the HD textures status does. Move it to HD textures · Ready to use. `features.md` is stale here too | `studio/src/hd_pane.js:499-500` |
| 108 | Scheduled Commands belong on a clip | They're offsets from every highlight, so they're project-wide (decision 4) | `studio/index.html:719`, `studio/src/roll_floors.js:36-41` |
| 146 | Sides and `:lower` aren't drawn | Rename demos draws both now. Change the row to Drawn | `docs/ui_facelift/canvas-pages.md` (Rename demos, Chips) |
| 209 | "Match the player's own view" isn't drawn | It's drawn, in the wrong section. On the canvas it's under View, "First-person demos only". In the game it's the first SPECTATING row and acts on HLTV demos. Move it to Spectating | `goldsrc-hooks/src/settings_tab.rs:139` |
| 15 | One close prompt | Two: batch running, and batch not started yet, which cancels the batch and closes the game | `studio/src/strings.js:1704-1718` |
| 22 | A chip on the unreadable demo's row | These demos never enter the queue, so there's no row. Use an end-of-scan line that opens a list | `studio/src/queue_scan.js:73-76` |
| 36 | Reuse "Speed up quiet gaps" for Review | They're two settings: Review defaults on, capture defaults to Keep. Reusing the switch flips Review's default | `studio/src/review_settings.js:10-11`, `studio/src/kill_gaps_setting.js:9` |
| 90 | Four OBS qualities | Five, with "OBS's own" | `studio/index.html:488-495` |
| 130 | "their own · one picture · none" for everyone else | HLTV demos have no "their own" choice. There are three rows: POV spray, HLTV spray, crosshair | `studio/index.html:927-950` |
| 138 | "Always keep copies in…" | A one-shot bulk pick, not a setting. It moves nothing | `studio/index.html:1184`, `studio/src/auditor_pane.js:41` |
| 159 | Chat chips All · Team · System | Also alive or dead and a team picker | `studio/src/analyzer_report.js:972,1027-1035` |
| 169 | Type chips | Six file types, so they belong behind Custom | `studio/index.html:1450-1458` |
| 174, 175 | Alt-click, and a click menu | Both are mode buttons beside Area and Label. Alt only outlines pieces | `studio/index.html:1650-1653,1721` |
| 183 | All rows need space | The install pick needs none with one install, and the map filter can be type-ahead on "+ Map" | `studio/index.html:1640-1642` |
| 193 | Ticks and Select unrendered | Leaves out "Select finished" and its age list | `studio/index.html:1051-1057` |

### Drawn rows that are out of date

- **#188 is no longer drawn.** It's marked Drawn as "Failed: game closed · Capture again", but fixing contradiction 4 removed that from every clip. "Capture the rest?" (`studio/src/batch_resume.js`, `studio/src/strings.js:1732`) is now on no page. Move it to Missing, and draw it as a pop-up.
- **#131** still says "96 px". The canvas now shows 64, which is the default (`studio/src/players.js:33`).
- **#106** lists a preset the app doesn't have. "The game's own" isn't a built-in (`studio/src-tauri/src/builtin_profiles.rs`). As a "no preset" chip it's fine, but mark it as new.

### Contradictions

| # | README status | Actual | Detail |
|---|---|---|---|
| 1 | Fixed | Fixed | Default is Frame sequence (`native/src/patch/types.rs:303`) |
| 2 | Fixed | Fixed | It does delete the oldest first (`studio/src-tauri/src/take_cleanup_manager.rs:364`). See decision 9 and G8 |
| 3 | Fixed | Fixed, one open choice | The drawer's switch is now honest: it's #450's second pass, which isn't built, so it belongs in item 19. Decide whether a variant needs both it and the "AGR for Blender" capture mode. Dropping the switch gives up "also record AGR", and dropping the mode means a variant can't be AGR only |
| 4 | Fixed | Fixed, with a side effect | See #188 above. `failed` is still in the page's status list, unused |
| 5 | Fixed | Fixed | |
| 6, 12, 17 | Deliberate | Agreed | |
| 7 | Fixed | Canvas fixed | `simplify-review.md` still says spray size becomes automatic (decision 8) |
| 8 | Fixed | Fixed | The canvas's template is valid, but isn't the app's default (`{map}_{name}_{kills}k_{weapons}_{time}`, `studio/src/clip_name.js:23`). Fine if that's a chosen change |
| 9 | Fixed | Fixed | |
| 10 | Fixed | **Not fixed, and the item itself is partly wrong** | The real Clean clips also hides the headshot red screen, screen shake, hints, map text and centre text, and sets `r_decals 256` and `mirv_fov 90` (`studio/src-tauri/src/builtin_profiles.rs:30-58`). So its "leaves everything else at the game's own" is false, and `features.md` has the same gap. Item 10 lists the hint box and centre text as canvas extras, but they're real. On the canvas, with Clean clips picked, red flash, screen shake and map text are shown Off where the profile has them on, victory music is On where the profile has it off, and "Hide teammate name" and "Hide server plugin text" are On where the profile doesn't set them and both default to off (`goldsrc-hooks/src/commands/install.rs:186,189`) |
| 11 | Fixed | **Half fixed** | X-ray is right. "Your own blood" has the right choices, but its hint says "hide a little, most, nearly all, or all". The setting does the opposite: it *shows* more of the player's own blood (`goldsrc-hooks/src/settings_tab.rs:98-104,278-281`) |
| 13, 14, 15, 16 | Fixed | Fixed | On 14: Setup calls FFmpeg "optional", but Render and the default Capture and render need it, so say "needed to render" |
| 18 | Fixed | **Moved, not fixed** | Settings now says Teams go in project settings, and Project settings has no Teams card. Teams already are project data (`studio/src/project_teams.js`). See decision 2 |
| 19 | New ideas | **Incomplete** | It leaves out the two largest: variants don't exist in the app (no use of the word in `studio/src/strings.js`; #45 is open), and Keep setting a clip to Pending changes behaviour, since today no review answer touches Status (`docs/features.md` 2.2). Map lighting is half real: light files work (`ds_map_apply_lighting`, `goldsrc-hooks/src/settings_tab.rs:324`), and only the page is new. Also new: the 3+, 4+ and 5+ kill chips and "Any weapon" on Demos, "Start OBS when a batch starts" (the app always tries, `studio/src/capture_pane.js:1161`), and #450's second AGR pass |

### New contradictions

- **Setup's "Safe for online play" is false and could cost an account.** The canvas says the hook DLL "only goes into the copy, never into the game you play on servers", and that captures "never touch the game you play online". A movie copy is a folder beside `dod` in the same install, and the DLLs go into `hl.exe` at launch, so the risk follows what runs in the game, not the folder (`docs/vac_safety.md:6-8,27`, #773, `studio/src/strings.js:521`). Replace the card with "Keeps your movie configs, models and sounds apart", and show the real rule in Setup: "Never join a server from a game DoD Studio started." It's the same rule as G5.
- **"Same headings and names as the in-game Settings tab" (Clip look) is false.** Examples, canvas then game: "Show the HUD" and "Draw the HUD"; "Use my map lighting" and "Apply map light files"; "Mute flag announcements" and "Mute capture announcements"; "Restore missing gunshots" and "Play gunshots a demo left out"; "Hide the headshot red flash" and "No red screen on head hits". Overview icons are under HUD in the game and Players on the canvas. Rename the rows or drop the claim, and settle it together with #899, which redoes that tab.
- **"Overview map icons" has no HLTV note.** The overview never shows in POV demos (#897), so the row should grey like the Spectating rows.
- **"Works through the hook DLL while capturing" undersells it.** The hook is also in the game for Watch in game and Review, and the "Changes in the game save here" switch relies on that.
- **`simplify-review.md` still names an Exports page and boards E and F**, which the canvas no longer has. "Custom" now sits in Project settings, and Export is a menu.
- **`ui-map.md` is from 2026-10-10.** It predates the Overviews, HD and Blender tabs and quotes 1,051 lines for `index.html`, which now has 2,285. Generate All Previews and Min Kills, which it describes, are no longer in `index.html` or `strings.js`.
- **`coverage.md`'s header counts** will change: #146 and #209 leave Missing, and #188 joins it.

## 3. Every Missing row

Check: OK, PARTLY or WRONG, as in section 2. Rec is Add, Automatic, Drop, Move or Already drawn. † marks a decision for Chris. Dropped code stays unless the row says otherwise: an in-game or console setting stays reachable there.

### App and projects

| # | Item | Check | Rec | Where or why | Proof |
|---|---|---|---|---|---|
| 1 | File menu: New / Load / Save Session (Ctrl+N/O/S) | OK | Add | Project switcher pop-up: New / Open / Save with Ctrl+N/O/S beside them. Ctrl+W, F5, Ctrl+R are swallowed (app_shortcuts.js:16-21), keep that | `studio/src/app_shortcuts.js:23-25`, `studio/index.html:28-39` |
| 2 | Open Recent and Clear Recent | OK | Add | Same switcher. Missing file is dimmed "(missing)"; clicking it asks Remove / Keep (no per-row Remove button today) | `studio/src/recent_projects.js:72-77, 87-94, 140-150` |
| 3 | Rescan Recent Sessions… | OK | Add | Keep the once-per-update pop-up, which backs each session up first (`studio/src/rescan_recent.js:1-11,25`). Drop only the File menu line | `studio/index.html:43`, `studio/src/rescan_recent.js` |
| 4 | Switch a .json project to .dodstudio (Switch / Keep as .json) | OK | Add | One-time pop-up after opening a .json project. Never offered for `.autosave.json` or when `.dodstudio` exists (project_convert.js:12-14) | `studio/src/project_convert.js:1-14` |
| 6 | Help → Check for Updates | OK | Add | Settings (this PC): "Check now" beside the Updates chips | `studio/index.html:51`, `studio/src/updater_pane.js:31` |
| 8 | Help → View Logs | OK | Add | Settings (this PC), App card: "Open activity log" | `studio/index.html:54`, `studio/src/main.js:246` |
| 10 | Setup checklist in place of the empty queue | OK | Add | Demos: the card stands in for the empty queue only when the queue has 0 demos; with demos it sits above the list. Also make Start greyed with "Finish setup" tooltip (see G2) | `studio/src/setup_wiring.js:131-134` |
| 12 | Update <version> available / Restart to update button | OK | Add | Top-bar chip at the right end, hidden with no update. Re-checks every 4 h (`update_chip.js:34`) | `studio/index.html:60`, `studio/src/update_chip.js:15-21` |
| 14 | Unsaved-changes prompt on close | OK | Add | Pop-up. Real buttons are Save & Close / Close Without Saving / Cancel. Needs the "untitled" and "unsaved" states shown (G12) | `studio/src/close_guard.js:27-33`, `strings.js:1695-1701` |
| 15 | Close while a batch runs prompt | PARTLY | Add | Two cases, not one: batch started in the game ("keeps capturing") and batch NOT yet started ("Closing cancels the batch and closes the game", Cancel batch and close). See section 2 | `studio/src/batch_close_prompt.js:1-18`, `strings.js:1704-1718` |
| 206 | Offer to rescan demos an older version scanned | OK | Add | Pop-up on project open, lists demos, shows a time estimate, "Not now" asks again next open. Carries marks by kills (rescan_carry.js) | `studio/src/project_rescan.js:22-45`, `strings.js (MAIN.OUTDATED_DEMOS_*)` |

### Demos

| # | Item | Check | Rec | Where or why | Proof |
|---|---|---|---|---|---|
| 19 | Include subfolders | OK | Add | Switch under + Add demos, default on, 6 levels (tooltip) | `studio/index.html:107-110` |
| 20 | Scan progress line and Cancel Scan (incl. "Looking for demos") | OK | Add | Thin bar over the list with step lines (looking for demos, checking which are new, reading N of M, time left) and Cancel | `studio/index.html:111-118`, `studio/src/queue_scan.js:70-100` |
| 21 | Recent and pinned folders, one-time pinned-folders review | OK | Add | Analyzer: Pinned and Recent, in the folder drop-down (#151). The one-time pinned review is a migration for old builds: Drop it | `studio/src/demo_folders.js:30`, `studio/src/pinned_review.js:1-8` |
| 22 | Unreadable demos named with a reason | PARTLY | Add | Demos: end-of-scan line "N couldn't be read" that opens a pop-up listing every file and reason | `studio/src/queue_scan.js:73-76`, `strings.js:2093-2097` |
| 24 | Search box (name, path or map) with × and Esc | OK | Add | Search box above the list; matches name, path or map; × and Esc clear | `studio/src/master_pane.js:200-205, 234-243`, `index.html:200` |
| 25 | Player filter: POV demos one player recorded | OK | Add | "Recorded by" chip/dropdown; hide it when the project has fewer than 2 recorders (it is built from the queue's recorders) | `studio/src/master_pane.js:166-190, 246`, `index.html:201` |
| 26 | Quick filters: with kills, multi-kill, headshot, POV only | OK | Add | One filter chip row. "POV only" means "has a resolvable recording player", i.e. hides HLTV demos without one (queue_filters.js:57-58) | `studio/src/queue_filters.js:38-46, 56-65`, `index.html:204-213` |
| 27 | Tracked only | OK | Add | Chip "Has my work" | `studio/src/queue_filters.js:59`, `index.html:215` |
| 28 | Group by match, player, map, folder, date, demo type | OK | Add | Group-by chips None / Match / Player / Map; Folder, Date, Demo type behind "More…" | `studio/index.html:131-139`, `studio/src/queue_group_by.js` |
| 29 | Sort by (order added, name, newest, oldest, most highlights) | OK | Drop | † Newest first becomes fixed. Search and grouping cover the rest | `studio/index.html:144-150`, `studio/src/queue_sort_by.js` |
| 30 | Right-click row menu (preview, review, open in Analyzer, open folder, split, locate, copy name/path, remove) | OK | Add | "⋯" on each row with the same items (also reachable by the Menu key / Shift+F10, master_pane.js:659-668). Missing demo dims items with a reason (demo_menu_items.js:16-19). Remove asks first when the demo has work on it (`studio/src/master_pane.js:582`) | `studio/src/demo_menu_items.js:16-35`, `studio/src/master_pane.js:654` |
| 38 | Clear Untracked / Clear Selected / Clear All | OK | Add | "⋯" on the list: Remove demos with no work / Remove ticked / Remove all. Remove all opens a save-first modal (clear-all-modal), keep it. Remove ticked and Remove keep the "has tracked work" confirm, with the count (`studio/src/queue_actions.js:92`) | `studio/index.html:187-189, 1987-1994` |
| 39 | Teams list (tags, display name, Same team as, Read Their Teams, Played as) | OK | Add | † "Teams" link on Demos opening a pop-up. Read Their Teams folds into the rescan offer | `studio/index.html:127`, `strings.js:1346-1366`, `studio/src/teams_pane.js` |
| 40 | Missing demo: Locate… / Use found copy | OK | Add | Row chip "Moved" with Find it / Use the copy found. Also project-open prompt and toast action "Use all found copies" (G3) | `studio/src/master_pane.js:505-512`, `studio/src/demo_menu_items.js:24-25`, `studio/src/project_demos.js:84` |
| 41 | Multi-map demo "2 maps · Split" in the queue | OK | Add | Row chip "2 maps · Split". Also stops capture (G1) | `studio/src/master_pane.js:523`, `studio/src/queue_multimap.js:29-37` |
| 42 | Maps needed banner with Download | OK | Add | Banner with Download / Download all / Dismiss; states missing, different build, unreadable | `studio/index.html:222`, `studio/src/map_warnings.js:100-123` |
| 43 | No camera data / trimmed copy badges | OK | Add | Chips on the row. "Trimmed copy" only exists on the pre-Anniversary engine (limit 256, packet_entity_limit.js:14), so show it only for that install | `studio/src/camera_data.js:36-43`, `studio/src/packet_entity_limit.js:14, 46`, `studio/src/master_pane.js:515-520` |
| 49 | Flag chip: Flag defence | OK | Add | Same chip style. Note: tag is the killer's side: victim stood in a flag area = "Flag defence" | `studio/src/flag_tags.js:15, 33-42` |
| 60 | View Match Telemetry | OK | Drop | Same action as "Open in Demo Analyzer" in the row menu (`demo_menu_items.js:20`), so the "⋯" menu carries it | `studio/index.html:265`, `studio/src/detail_pane.js:380-392` |

### Review and the game

| # | Item | Check | Rec | Where or why | Proof |
|---|---|---|---|---|---|
| 32 | Review: which marks to play (All / Unmarked / Keep / Maybe / Skip) | OK | Add | Chips beside Review in game. Persisted (review_covers) | `studio/index.html:154-160`, `studio/src/review_settings.js:6-12` |
| 33 | Review: in-game answer keys (Y M N R B Space S, E form) | OK | Drop | From Studio's page: the in-game box already lists the keys (review_keys.rs:1-8). One line in the button tooltip is enough. Keys also include X (X-ray) and E (form) | `goldsrc-hooks/src/review_keys.rs:13-23` |
| 34 | Review: HLTV whose highlights (missing POV, recorders, everyone, picked players/teams) and play order | OK | Add | Show only when the queue holds an HLTV demo. Default "Missing their POV" is automatic; "Everyone / Pick…" behind a chip; order By player behind Pick… | `studio/index.html:161-173`, `strings.js:2846-2852`, `studio/src/review_hltv_picker.js` |
| 36 | Review: fast-forward gaps over N s | PARTLY | Automatic | Review always fast-forwards gaps over 8 s, today's default, with no control. Separate from the project's Speed up quiet gaps, which stays off by default. See section 2 | `studio/src/review_settings.js:10-11`, `studio/src/kill_gaps_setting.js:7-9, 14` |
| 37 | Saved review marks library ("Bring them in / Not now") | OK | Add | Pop-up as drawn; lists the demos and carries a footer on what it will not overwrite | `studio/src/review_library.js:224-236` |
| 59 | Launch Game | OK | Drop | † Watch in game, Review and Capture already start the game. Keep the code for a dev menu | `studio/index.html:262`, `studio/src/capture_pane.js:505` |
| 61 | Game already running prompts (Force Relaunch / Copy View Command / Cancel; other install or hook build) | OK | Add | TWO prompts: (a) engine already running, Force Relaunch / Copy View Command / Cancel; (b) running with other settings, Close and start again / Cancel. Merge into one pop-up; drop Copy View Command (preview only, not offered for a batch: `detail_pane.js:118`) | `studio/src/running_game_guard.js:49-60`, `studio/src/detail_pane.js:77-100, 290-325`, `strings.js:334-346, 2658` |
| 65 | Steam running / signed-in check | OK | Add | Pop-up Start Steam / Cancel; waits up to 120 s for sign-in (steam_guard.js:14) with a status line | `studio/src/steam_guard.js:1-30` |
| 66 | Game or HLAE error box explained | OK | Add | Pop-up with a plain reason. Preview / Launch Game errors come as a 15 s toast from the dialog watcher; a batch reports through its own status (capture_pane.js:936-940) | `studio/src/capture_pane.js:882-890`, `studio/src-tauri/src/game_process_manager.rs:55, 94` |
| 205 | In-game DoD Studio window, review overlay and keys, From the start / From here | OK | Drop | The in-game window is a separate UI (vgui). Track as its own task. Only Studio-side bits need a place: Review button + the "Review running" state (G16) | `goldsrc-hooks/src/review_keys.rs:1-60`, `goldsrc-hooks/src/review.rs:43-50` |
| 220 | DoD Studio window rows (viewdemo opens window, console key, resizable, remember layout, folder lists, counts, hide empty, count subfolders, review keys, review info) | OK | Drop | Window rows live in the in-game tab. Review keys and review info: one line on Demos next to Review in game is cheap but not needed | `goldsrc-hooks/src/settings_tab.rs:397-442` |

### Clips and capture

| # | Item | Check | Rec | Where or why | Proof |
|---|---|---|---|---|---|
| 53 | Status set by hand (✎ and Undo toast) | OK | Add | Click the status chip to pick; pencil mark; Undo toast is worth keeping (detail_pane.js:610-617) | `studio/src/detail_pane.js:506-508, 602-620` |
| 54 | Notes per highlight | OK | Add | Note line in the variant drawer / expanded row, not on the Clips row | `studio/src/detail_pane.js:550, 642-651` |
| 55 | Typed clip name (wins over the template) | OK | Add | Click-to-edit name; grey placeholder = automatic name (detail_pane.js:626) | `studio/src/detail_pane.js:539, 624-636` |
| 57 | Capture history per highlight (Details column) | OK | Add | Expand a row; includes renders of the take and videos of deleted takes (#1264) | `studio/src/capture_history.js:1-10`, `detail_pane.js:657` |
| 64 | Disk space needed vs free, and why Start is disabled | OK | Add | Footer "Needs N GB, M free" and a greyed Start. See G2 for the other reasons that grey Start | `studio/src/capture_pane.js:206-346, 357-405` |
| 68 | Last batch panel (takes on disk, gaps at 4x) | OK | Add | End state of Clips; shows each take, size, folder, and cut-short takes (strings.js:2509-2515) | `studio/src/batch_results.js:1-12` |
| 69 | OBS drop-rate warning | OK | Add | OBS mode only. Yellow line plus a one-time toast per batch; per-clip totals go to Last batch | `studio/src/obs_drop_rate.js:1-12, 34-60`, `capture_pane.js:741` |
| 70 | OBS still recording from a crashed session | OK | Add | Pop-up at start-up, only in OBS mode and only if the recording is Studio's own | `studio/src/config_wiring.js:114-143`, `studio/src/main.js:429` |
| 121 | Render while capturing | OK | Add | Clips, Capture menu: third item "Render each clip as it is captured". Off by default; its "waiting for room" status line (render_while_capturing.js + capture_pane showCaptureRoomLine) must show on the capturing screen. With decision 9's chips, it appears only with "Is deleted when space runs low", and `HINT_ON_DELETE` needs new words | `studio/index.html:883`, `studio/src/render_while_capturing.js:1-20` |
| 189 | "Batch may still be capturing" wait and Start refusal | OK | Add | Banner on Clips + Start greyed with tooltip (today Start is clickable and refuses with a status line, against the greyed-with-reason rule) | `studio/src/batch_catch_up.js:178`, `studio/src/capture_pane.js:1146`, `strings.js:1725-1727` |
| 190 | Scan for Takes | OK | Automatic | Scan when Clips opens and when a batch ends. Keep one "Look for takes" link; Import takes pop-up covers other folders. Not during a batch or a render, silent when no folder is set, and with its existing progress line | `studio/src/render_pane.js:930` |
| 191 | Render row details (codec, FPS, speed, progress, size) | OK | Add | Under the status while rendering: "ProRes 422 · 2.1x · 64%"; size once done | `studio/src/render_pane.js:361-368 (stream, frames, speed, progress, size), 306` |
| 192 | Row actions: Cancel, Retry/Reset, Remove, View Log, Open Folder | OK | Add | Row "⋯" menu. Cancel and Retry only appear in the matching state. View log only when failed | `studio/src/render_pane.js:196-224` |
| 193 | Tick boxes, Select unrendered, bulk Cancel/Retry/Remove/Delete, Clear Queue | PARTLY | Add | Tick bar. Row omits "Select finished" with its age list (any / over a day / week / month), the way to clear space (index.html:1051-1057). Clear Queue has a confirm (render_pane.js:1098) that must stay | `studio/src/render_pane.js:707-710` |
| 194 | HUD-only row (ProRes 4444 with alpha) | OK | Automatic | A "HUD layer" sub-row appears only when the capture made a HUD stream. Canvas Export menu already says "HUD on its own track" | `studio/src/render_pane.js:364 (Stream column)`, `native/src/hlcr/renderer/ffmpeg_args.rs:505 (`hud_only`, 4444)` |
| 195 | Render history per take | OK | Add | Expanded row, with capture history (capture_history.js:1-10 already lists renders of a take) | `studio/src/render_history.js:53`, `studio/index.html:1078` |
| 198 | Add Nameplate Script to VEGAS | OK | Add | Export menu, fourth item. Rare, so one line. Add a note it needs VEGAS | `studio/src/vegas_script_install.js:13` |
| 199 | Render crash recovery (Recover / Discard) | OK | Add | Pop-up on start-up. Must stay: it prevents lost renders | `studio/src/render_pane.js:615-625`, `studio/index.html:2098-2106` |
| 202 | Take pictures: middle-frame thumbnail on each row | OK | Add | Small picture on each captured row; click opens the drawn nine-frame page | `studio/src/take_preview.js:169`, `studio/src/render_pane.js:316` |
| 204 | Delete Take | OK | Add | "⋯" menu: "Move take to Recycle Bin" with the size confirm. Greyed with the reason while it renders (strings.js DELETE_TAKE_RENDERING_TITLE) | `studio/src/render_pane.js:224`, `studio/src/take_delete.js:44 (confirm with size)` |

### Settings and Project settings

| # | Item | Check | Rec | Where or why | Proof |
|---|---|---|---|---|---|
| 78 | 2 GB / 4 GB memory report for hl.exe | WRONG | Add | HD textures "Ready to use?" card: row "Game memory: 4 GB / 2 GB (HD textures can run it out)". Not on Paths today | `studio/src/hd_pane.js:499` |
| 80 | Size the game's menus like the pre-Anniversary engine | OK | Add | Settings, Game card: switch "Normal-size menus", shown only for an Anniversary install; row greyed with the reason when a foreign scheme file is there (line 24) | `studio/src/menu_scale_setting.js:19`, `studio/index.html:319` |
| 84 | AGR FPS | OK | Automatic | AGR rate = Smoothness. Empty field already means "same as Capture FPS" (config_wiring.js:189) | `studio/index.html:418`, `studio/src/config_wiring.js:186` |
| 88 | OBS Test Connection button | OK | Add | Settings, OBS card: "Test connection" button (it also builds the DoD-Studio profile/scene), beside the Connected status. Canvas has only a hint | `studio/index.html:475` |
| 89 | OBS Capture FPS | OK | Add | Project settings: Smoothness chips set the OBS rate when Capture with = OBS (a separate stored value, default 120, hidden Capture FPS in OBS mode) | `studio/index.html:481`, `studio/src/config_wiring.js:181` |
| 90 | OBS Quality (Small / Standard / High / Max) | PARTLY | Add | Settings, OBS card: chips Small/Standard/High/Max with GB per minute; "OBS's own" goes behind Custom (five choices, not four) | `studio/index.html:488-495` |
| 95 | Live timeline diagram | OK | Add | Real thing is an events table, not a bar. Replace with one text line under the two clip-length fields: "5 s, first kill ... last kill, 5 s" | `studio/index.html:617` |
| 101 | Clear Previews… | OK | Automatic | Watch in game previews, where the game can't play the demo in place, still leave `_preview.dem` files, each with a sidecar marker. Sweep them quietly after a batch or at start, never while the game is open, and toast "Removed 6 old preview demos (1.2 GB)" | `studio/src/orphaned_previews_modal.js:77`, `studio/index.html:659` |
| 118 | Render Presets | OK | Drop | With Source FPS and concurrency cut, a preset is a codec, plus custom FFmpeg args when the codec is Custom. The Finished clips chips hold the codec, and Custom holds one set of args. Map old presets into the project once | `studio/src/render_presets.js:7-17` |
| 123 | Clip-name placeholder chips | OK | Add | Project settings: placeholder chips under "Name clips like", same style as Rename demos | `studio/index.html:895` |

### Clip look

| # | Item | Check | Rec | Where or why | Proof |
|---|---|---|---|---|---|
| 104 | Field of view (first-run mirv_fov 90; r_decals 256 default) | OK | Add | † Top of Clip look: 90 · 100 · Custom, starting from Clean clips' 90 | `studio/src-tauri/src/builtin_profiles.rs:32` |
| 105 | Import Config… | OK | Add | Clip look, under Your own commands: small link "Import a .cfg..." (read-only, file picker) | `studio/src/capture_pane.js:558`, `studio/index.html:716` |
| 107 | Profile Save Changes / Rename / Delete | OK | Add | Clip look: "..." on your own preset chip: Rename, Delete, Save changes. Built-ins show only Duplicate | `studio/index.html:697`, `studio/src/command_profiles_ui.js:230` |
| 108 | Scheduled Commands | WRONG | Add | † Clip look: second box "At each clip: command, seconds, before or after". Project-wide, not on a clip. The rest-of-batch warning moves with the box: the first clip records at one value and later clips at another (`studio/src/strings.js:1790,1834`) | `studio/index.html:719`, `studio/src/roll_floors.js:36-41` |
| 109 | Console-name autocomplete | OK | Add | Clip look: suggestion list under the Commands box; it also carries the tier notes (refused / refused when scheduled / warned / no-op), which the canvas hint reduces to "refused" | `studio/src/command_suggest.js:105,131` |
| 111 | Your game configs' warnings (config.cfg, exec followed) | OK | Add | Warns when movie.cfg/config.cfg sets something the pipeline relies on (a capture once ran at mirv_fov 105 unnoticed). Add a yellow note at the top of Clip look and repeat it as a pop-up at Capture. Prevents a wrong capture, so it must not be dropped. Every kind in `STRINGS.CFG` stays | `studio/src/cfg_warnings.js:1-20`, `studio/index.html:713` |
| 209 | Match the player's own view (spec_match_pov) | WRONG | Move | Clip look: from View to Spectating, greyed for a first-person demo | `goldsrc-hooks/src/settings_tab.rs:139`, docs/ui_facelift/canvas-pages.md:45 ("Match the player's own view" under View) |
| 210 | Overview icon (hud_overview_team_marker) | OK | Add | Second dropdown on the "Overview map icons" row | `goldsrc-hooks/src/settings_tab.rs:240` |
| 211 | No speaker icons on the overview (hud_overview_hide_speaker) | OK | Drop | Stays in the in-game tab and console. Cosmetic | `goldsrc-hooks/src/settings_tab.rs:246` |
| 212 | Stop models flickering (player_fix_model_flicker) | OK | Automatic | No switch: it's on by default (`goldsrc-hooks/src/strafe_twitch.rs:63`). Add it to Clean clips only so the preset sets every look setting | `goldsrc-hooks/src/settings_tab.rs:296` |
| 213 | Run per-map cfgs (map_run_cfgs) | OK | Add | Map: "Run my map .cfg files". Only people with dodstudio_maps cfgs need it. Show it with a note when such files exist, else hide | `goldsrc-hooks/src/settings_tab.rs:319` |
| 214 | HD textures on/off (hd_enabled) | OK | Add | Add "Off" as the first choice of the HD textures box | `goldsrc-hooks/src/settings_tab.rs:454` |
| 215 | Seeking skips what it jumps over (demo_seek_skip_between) | OK | Drop | Stays in the in-game tab. Off by default | `goldsrc-hooks/src/settings_tab.rs:363` |
| 216 | Open demos on other drives (demo_other_drives) | OK | Automatic | Studio sets it when the demo is on another drive | `goldsrc-hooks/src/settings_tab.rs:368`, `native/src/preview_in_place.rs:40 (`OTHER_DRIVES_ON`)` |
| 217 | Show how far a demo has loaded (demo_show_load_progress) | OK | Drop | Default is on (goldsrc-hooks/src/demo_bar_progress.rs:33). In-game tab only | `goldsrc-hooks/src/settings_tab.rs:373` |
| 218 | Fast loading (demo_fast_load) | OK | Drop | Default is 8 ms. In-game tab only | `goldsrc-hooks/src/settings_tab.rs:378` |
| 219 | ex_interp up to (demo_ex_interp_max) | OK | Drop | Stays in the in-game tab, stock 100 ms stays | `goldsrc-hooks/src/settings_tab.rs:384` |

### Players and sprays

| # | Item | Check | Rec | Where or why | Proof |
|---|---|---|---|---|---|
| 128 | Crosshair sprite and 4x4 tile pick | OK | Add | Pop-up from the Crosshair cell: 4x4 tile grid. Say in it that a POV demo needs the recorder's cl_xhair_style at 1-16 (strings.js:3383) | `studio/src/players_ui.js:53,305`, `studio/src/strings.js:3352` |
| 129 | Make your own crosshair… / Add a crosshair file… | OK | Add | Players and sprays: buttons "Make a crosshair" (opens Vertex's web maker, crosshair_file.js:10) and "Add a crosshair file" | `studio/src/crosshair_import_ui.js:24,110` |
| 130 | Fallbacks for players not listed (POV spray, HLTV spray, crosshair) | PARTLY | Add | Players and sprays, Options: three rows. Spray in POV demos: their own / one picture / none. Spray in HLTV demos: one picture / none (no "own" choice). Crosshair: game's own / one crosshair. The canvas "Fallback" label per row has no control behind it | `studio/index.html:927-950` |

### Auditor

| # | Item | Check | Rec | Where or why | Proof |
|---|---|---|---|---|---|
| 136 | "Not an exact copy" mark | OK | Add | Row chip "Not an exact copy", shown after a delete compared bytes and found a difference. Result state, not a control | `studio/src/auditor_pane.js:308`, `studio/src/strings.js:1295` |
| 137 | Pick the kept copy, Reset | OK | Add | Click Keep on another copy; group shows "Kept copy changed by you" and Reset | `studio/src/auditor_pane.js:257` |
| 138 | Keep copies already in a folder… | PARTLY | Add | It is a one-shot action ("Keep copies already in a folder..."), enabled only after a scan, that picks the kept copy in every group at once and moves nothing. Not an "always keep" setting. Put the button in the Found card | `studio/index.html:1184`, `studio/src/auditor_pane.js:41` |
| 139 | Open Recycle Bin, Copy Path, Open Folder | OK | Add | "..." per row: Copy path, Open folder. After a delete, toast "Moved 14 to the Recycle Bin" with an Open Recycle Bin button | `studio/src/auditor_pane.js:52,370,382` |
| 140 | Progress bar and step line | OK | Add | Bar plus one line under Found. Four real steps: listing folders, looking inside archives, comparing demos, checking each copy byte for byte. Same bar for delete | `studio/src/audit_duplicates.js:75,102` |
| 146 | Rename chips: Sides ({faction}, {enemy_faction}) and :lower | WRONG | Already drawn | Change the row to Drawn | docs/ui_facelift/canvas-pages.md (Rename demos, Chips card: "Sides {faction} {enemy_faction}" and "Tricks :first :upper :lower /fallback"), `studio/src/demo_rename.js:31` |
| 150 | Default (reset template) buttons | OK | Add | Small "Default" link beside each template (and beside "Name clips like"). Cheap | `studio/index.html:1249`, `studio/src/strings.js:3178` |

### Analyzer and website

| # | Item | Check | Rec | Where or why | Proof |
|---|---|---|---|---|---|
| 151 | Explorer sidebar (Pinned, Recent, Local, tree, folder counts, Add Pin, Refresh, resizable) | OK | Add | † Analyzer: folder drop-down (Recent, Pinned) plus dragging a folder in. Tree, counts, Refresh and the resize handle go | `studio/index.html:1793-1810` |
| 153 | Demos filters (text, type, map, date range, player) | OK | Add | Search box plus Type chips (All / First-person / Spectator); Map, Player and Date behind one "Filter" chip. All five filters exist | `studio/index.html:1823-1836` |
| 155 | Multi-map notice with Split now | OK | Add | Banner above the report with "Split now" and a progress bar (split_progress.js). Works on this one demo, not the Auditor page | `studio/src/analyzer_multimap.js:1-12`, `studio/index.html:1873` |
| 156 | Damaged demo: how far it was read | OK | Add | Banner: "Read up to 14:20, 3.1 MB unread". Also the third banner, "no camera data" (studio/src/analyzer_pane.js:117-128), should be drawn with it | `studio/src/analyzer_damage.js:1-12` |
| 159 | Chat Log filters (team, alive/dead, system) | PARTLY | Add | Chat filters are All chat, Team chat, Alive/Dead/All, a team picker and System categories (analyzer_report.js:972), plus Select/Clear all. A three-chip row loses alive/dead and the team picker | `studio/src/analyzer_report.js:1027-1035` |
| 160 | Copy kill-feed command, copyable Steam IDs | OK | Add | Copy buttons next to each SteamID (three forms) and "copy show-only command" on the player | `studio/src/analyzer_report.js:90,97` |
| 208 | Web analyzer: several demos at once, JSON export | OK | Add | Website Analyzer: "Open demos…" takes many; list shows status per demo (View / JSON) and "Download all (JSON)" | `web-analyzer/www/index.html:17,22`, `web-analyzer/www/strings.js:208-221` |

### HD textures, Overviews, Blender

| # | Item | Check | Rec | Where or why | Proof |
|---|---|---|---|---|---|
| 167 | Point at your own Python / upscaler | OK | Add | HD textures: "Use my own…" link inside the "Set up" pop-up; shown only when Set up can't find them | `studio/index.html:1376,1381 (pick buttons)`, `studio/src/hd_pane.js:1219 (pickPath python)` |
| 168 | Maps to build (all, or picked maps and patterns) | OK | Add | HD textures, Build tab: "Maps: All · Pick…" with search. Note it is the install's hd_maps.txt, not per project | `studio/index.html:1404-1421`, `studio/src/hd_pane.js:820` |
| 169 | Build options: styles, file types, largest size | PARTLY | Add | Build tab: style chips and 1024 · 2048 · 4096 chips. File types (6 kinds) fold behind "Custom…", default all | `studio/index.html:1450-1458` |
| 174 | Recolour a single piece of floor | PARTLY | Add | Overviews: it is a mode button (Area / Piece / Hide / Label chips above the map), not an Alt-click. Alt only outlines pieces (index.html:1721) | `studio/index.html:1651 (button "Colour piece")`, `studio/src/overviews_pane.js:497` |
| 175 | Hide an area | PARTLY | Add | Same mode chips; the row's "click menu" is wrong | `studio/index.html:1652 "Hide area" mode`, `studio/src/overviews_pane.js:343` |
| 176 | Rename flags | OK | Add | Overviews: names list under Show, or click the label on the map | `studio/index.html:1756`, `studio/src/overviews_pane.js:64` |
| 177 | Add labels | OK | Add | Fourth mode chip "Label" beside Area / Piece / Hide | `studio/index.html:1761 ("Add label" mode)`, `studio/src/overviews_pane.js` |
| 180 | BMP format choice | OK | Add | Format chips TGA · BMP in "Save to" | `studio/index.html:1769-1771`, `studio/src/overviews_pane.js:66` |
| 183 | Theme, other colour / original colour, Undo, title card, credit, flag icons as seen at, map filter, install pick | PARTLY | Add | Undo and colour chips over the map. Title, credit, flag-icons rows in a "Title card" box. Install pick only when more than one install. Map filter only matters with many maps: Automatic (type-ahead on the "+ Map" tab) | `studio/index.html:1640 (install), 1642 (filter), 1654 (Undo), 1679 (theme), 1710-1714 (title, credit), 1726 (flag icons seen at)` |
| 186 | Choose blender.exe / find automatically | OK | Automatic | Find Blender by itself. Show "Browse…" on the Needs row when it isn't found or the found one isn't 4.4 ("other versions may fail at Import"), and name the import add-ons when they're missing (`studio/src/strings.js:2173-2176`) | `studio/index.html:1560-1561`, `studio/src/blender_pane.js:210-219` |
| 187 | Frames to render (optional) | OK | Add | Small box on the Render step, "Only frames…", shown when EEVEE preview / one Cycles frame is picked | `studio/index.html:1609`, `studio/src/blender_pane.js:151,291` |

## 4. What the redesign doesn't account for

### Workflows and edge cases

- **Scale.** The canvas shows 12 clips and 5 demos, and a real movie has hundreds: WSOD25 has 364 Keeps. Clips needs search, player and demo filters and grouping (G10). Keep an eye on row height, and draw one page at real size.
- **Variants are the largest piece of new work.** The design treats them as settled, but #45 is open and priority-low. A per-clip X-ray or crosshair needs per-clip commands, its own take and status, and a project file change.
- **Keep = in the project changes what's in the project.** Today every scanned highlight is a row. Maybe and Skip must read clearly as "not in the project" (a Maybe never ticks, and Skip locks a row out of capture, `studio/src/strings.js:304,2893`). Say what Maybe or Skip does to a clip that already has a take: keep the row and its take and grey it, or ask first. Today Skip leaves status and takes alone (`studio/src/detail_pane.js:477-481`).
- **Moving global settings into projects (#1085) has no plan on the canvas.** It needs: what a new project starts from, what an old project gets when opened, and where old render presets and players go. Draw a New project pop-up with "Start from: another project · Defaults". It also needs what a capture or Watch in game uses with no project open.
- **The install is per PC on the canvas, but a movie picks one.** #1085 lists the POST install among WSOD25's choices, and HD textures, light files and `-addons` content all live per install. #1085 proposes asking "This project was made with POST. Switch?", never switching silently. Draw that.
- **During a batch.** The canvas doesn't say whether Demos, Project settings or Clips can be edited while capturing, or what a change does to the running batch.
- **Window size, colour and themes.** Check the sidebar plus two-pane Demos at 1280 wide (#46, #1292). The main colour is undecided, and #49's light and dark themes need a place in Settings · App. The app has no theme setting today.

### Issues and PRs that need a place

| Issue or PR | Needs | On the canvas now |
|---|---|---|
| #1264, PR #1303 | Export a whole project, including videos whose takes are gone | Export menu says "Rendered clips in order". Reword it, and show deleted-take videos on Clips |
| #1265 | Premiere and Resolve exports | Add rows to the Export menu |
| #1039, #1051 | Motion blur and slow-motion versions from one take | No place. Add to "Finished clips" as extra versions per clip |
| #585 | Rough Cut page, with Combine as its export | No place. A Studio sidebar item if it's built |
| #876, PR #879 | Split status table per map | Split maps has no status column (G19) |
| #872, PR #874 | Consolidate demos into one folder | Not on Auditor. On hold for the Auditor review |
| #983, #1331 | Backup to a checked archive, and to a folder | Backups draws folders only |
| #1110, #621, PR #622 | More queue filters, highlight tags and scores | Demos has neither filters nor a score sort. Leave room in the filter row |
| #1014, #934 | Help pages, and report a problem | Help menu (G11) |
| #896, #1325 | Explain a good capture | Clip look's preview answers part of it. Picture, Clip length and Setup have none |
| #600, #444 | Fewer settings tabs, configuration profiles | The facelift replaces #600. #444 depends on decision 3 |
| #1322 | The recorder's own spray in POV clips, against their Players-table spray | Players options (#130), still undecided |
| #1085 | The folder under each drive, per project | Add to Project settings: "Folder under each drive: [name]". Capture folders in Settings hold drives only |
| #450 | "Also record AGR" as a second pass | Variant drawer's switch. Clips · capturing needs that pass as a row |
| #581 | Font, spawn names, a movable title, more themes | Overviews' title card (#183) needs room |
| #429, #1130 | Nameplate burn-in, in-game nameplates | Nameplate column. Add the burn-in choice if it's built |
| #1317 | Reload a light file in one step | Map lighting's "Preview in game" will need it |
| #668 | HD folder on another drive | Settings · Where files go, if built |
| #899, #785 | In-game window redesign | Settle names with Clip look. The window itself stays off this canvas |
| #1045, PR #1047 | Main-menu background on or off | Settings · Game, one switch if built |
| #478 | Live values and config warnings | Clip look: a yellow note (#111) and "Changes in the game save here" |
| PRs #1292, #1293 | Panels that fit and resize | Work on today's layout. Decide if they merge before the facelift |

Moments (#1326-#1330), take checks (#1084), previews (#1325), Backups (#1331) and flag chips (#1132) are already drawn.

## 5. Cuts in `simplify-review.md` that need a change

The cuts are mostly sound. Each of these loses something unless it's handled.

| Cut | What it would lose | Fix |
|---|---|---|
| #120 Finish clips automatically, one codec | OBS takes can be kept as recorded. One codec chip re-encodes every OBS clip for nothing (`studio/src/finish_clips.js:30-34`) | Automatic: keep OBS takes as recorded, and offer "Re-encode…" under Custom. This replaces the per-row Skip |
| #117 Automatic concurrent renders | GeForce cards allow only a few encoder sessions, so NVENC renders can fail on a many-core PC (`studio/src/render_pane.js:1124-1128`) | Cap at 3 for NVENC, and keep NVENC reachable under Custom |
| #79 One FFmpeg copy | HLAE finds FFmpeg only in its own folder or through `ffmpeg.ini`, so "one copy" works only if Studio writes that link (`native/src/shared/hlae_ffmpeg.rs:5-9`) | Studio writes the link, with the Windows permission prompt, and shows a failure (G6). When HLAE keeps its own copy or another ini, say so, as `HLAE_FFMPEG_DIVERGED` does today |
| #92, #94 Automatic pre-roll and post-roll | The sound resync, sound flush, decal sweep and Scheduled Command offsets must all fit (`studio/src/roll_floors.js:2-12`) | Compute from those terms, taking the top of the audio range, and test it, since the floor has never been enforced (`studio/src/roll_floors.js:10-13`) |
| #116 Source FPS | It's the fallback for older takes, and for imported frame folders (`studio/src/strings.js:870`) | Keep the fallback in code. Import takes reads the rate or asks |
| #73 Game Folder from the movie copy | Several game folders side by side lose the list | The install picker lists each folder with a `liblist.gam` |
| #99 Always behind your windows | The only way to watch a clip record | Decision 10 |
| #113 After a clip is finished | The "delete as soon as it renders" state | Decision 9 |
| Notification switches | The six have different defaults | One switch means captures, renders and errors only |
| #100 Debug switches | Auto-clear Temp Demos defaults on | Keep that default in code when the switch goes |
| #76 Hook DLL override | Right for players, and it's the developer's way to pick a test build (`studio/src/hook_dll_row.js:1-25`) | Keep the debug-only row |
