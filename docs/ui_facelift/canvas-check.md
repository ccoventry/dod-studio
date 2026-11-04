# Canvas final review: fixes for the design agent

Canvas version checked: `1793774263-0902`, unchanged from start to end. Page names are the board files in the artifact (`project/<PageName>.dc.html`). In "state sheet" below: ClipsStates, DemosStates, SettingsStates and ToolsStates. Pop-ups and the new pages count as drawn in their own right.

## Must fix

| Page | What's wrong | Proof | Exact fix |
|---|---|---|---|
| StudioCapturing, StudioRendering, StudioBatchDone | Header buttons "Stop after this clip" (the main button) and "Stop now" don't exist on either page, and aren't marked as a proposal. The app has one cancel per batch, plus Cancel on a render row. StudioBatchDone repeats it: "Stop, or the game closing, ended the take early". | `studio/src/strings.js:714` (Cancel Batch), `:904`, `:1023`, `:1060`; no "stop after" anywhere in `studio/src`; `studio/src/strings.js:781` (cut short "by Cancel") | StudioCapturing: one button "Cancel batch", with the line "Cancels the clip being recorded: its take is cut short." StudioRendering: one button "Cancel render batch". StudioBatchDone card: "Cancel, or the game closing, ended the take early." |
| PopStartChecks | The header's main "Start anyway" covers all five rows. Only the crash-map and no-camera prompts have it. More than one map, changed on disk and missing demo all stop the batch, and can't be started anyway. | `studio/src/strings.js:1991`, `:2057` ("Capture didn't start"); `studio/src/project_demos.js:134,142-151`; `studio/src/queue_split.js:107-131`; `studio/src/crash_map_warnings.js:39-45`; `studio/src/camera_data.js:28-32` | Split the list into two blocks. "Capture didn't start" (fix first): more than one map · Split them; changed on disk · Rescan; missing · Find it; no Start anyway, and a line "Press Capture again when fixed". "Asks first": crash map and no camera data · Start anyway · Cancel. Show the header's Start anyway only when the second block is the only one present. |
| SettingsStates | The Take row draws three different chips from AppSettings: "Is kept · Is deleted when space runs low · Is deleted for good when space runs low". There's no Recycle Bin chip, and two chips name one state, so the first reads as if it weren't permanent. In the app, "when space is needed" exists only with the permanent choice and always deletes for good. | `studio/src/take_cleanup.js:22,29-33`; `studio/src/strings.js:1120-1121`; `studio/index.html:748-755`; AppSettings row "After a clip is rendered, its take" | Use AppSettings' chips: "Stays · Goes to the Recycle Bin · Is deleted when space runs low". Change the "Delete for good" row to: "Is deleted when space runs low deletes for good. Picking it asks first." Keep the row "Deleted as soon as it renders · Gone · Undecided · Chris's call 9". |
| ClipLook | With Clean clips picked, two rows and the preset hint don't match it. "Overview map icons" shows "Everyone"; Clean clips sets 0, "The game's own". "Over each player" shows "Name and health bar"; Clean clips doesn't set it and it defaults to Off. The hint says Clean clips "sets every switch on this page", while at least eleven rows are not set by it and two of them say so. | `studio/src-tauri/src/builtin_profiles.rs:30-58`, line 46; `goldsrc-hooks/src/settings_tab.rs:85-86,118-123`; `goldsrc-hooks/src/commands/install.rs:257` | "Overview map icons": "The game's own". "Over each player": "Off" (the on value's label is "Name and health"). Hint: replace "sets every switch on this page" with "sets these, and leaves the other rows as they are". Replace "Match POV" with "Match the player's own view". |
| ToolPlayers, ToolsStates | Decision 8 is drawn on ToolPlayers with no chip ("Spray size · 64 px (the default)"). ToolsStates' chip reads "64 px · kept · Chris's call 8", with no "Undecided". | review.md decision 8 | ToolPlayers: add the chip "Undecided · Chris's call 8" beside Spray size. ToolsStates: "64 px · Undecided · Chris's call 8". |
| AppSettings | Decision 9 is drawn as its recommendation, with the delete-at-once state gone, and has no chip. ReviewCalls lists only the state sheet for it. | review.md decision 9; AppSettings row "After a clip is rendered, its take" | Add the chip "Undecided · Chris's call 9" to that row. |
| PopMovieCopy | "Also copy demos" is drawn On and the name "dod_movie". The defaults are off and `dod_movies`. AppSettings already shows `dod_movies`. | `studio/src/movie_copy_ui.js:214`; `studio/src/movie_copy.js:9`; `studio/src/strings.js:3503` | Draw Also copy demos as Off and the name as "dod_movies". |
| ToolLighting | The aside says "Only used while capturing through the hook DLL. Your normal game is untouched." "Only while capturing" is false: the page's own Preview in game uses it, and so do Watch in game and Review. "Your normal game is untouched" repeats the safety framing review.md removed from Setup: the DLL goes into `hl.exe` whichever folder the game plays from. | review.md "New contradictions" items 1 and 4; `docs/vac_safety.md:3-8,27-32` | Replace the aside with: "Applies in clips, Watch in game and Review." Don't say anything about the normal game being safe or untouched. |

## Should fix

| Page | What's wrong | Proof | Exact fix |
|---|---|---|---|
| AppSettings | The side note "Teams goes in each project's settings (#1085)" states decision 2 with no chip, and only half of the recommendation. | review.md decision 2 | "Teams: Demos → Teams, or Project settings", with "Undecided · Chris's call 2". |
| AppSetup | Step 1 "Movie copy of Day of Defeat" has a ✓ while its text says one still has to be made, and the page has no button that opens PopMovieCopy. "Make it in one click" is a card with nothing to click. | `studio/src/strings.js:3496,3500` | Step 1: title "Half-Life" with ✓, amber line "This is dod, the folder you play online with", outlined button "Make a movie copy…" linking to PopMovieCopy. |
| AppSetup, ToolBlender | Two filled main buttons on one screen ("Next: FFmpeg" and step "Run"; "Run next step" and step "Run"). "Run" says nothing about the step. | README design rules; `studio/src/strings.js:3475-3482` | Make step buttons outlined. Label them "Check" (FFmpeg) and "Build scene" (Blender). |
| ClipLook | "The game's own" preset chip is unmarked, though the app has no such preset. | review.md section 2, #106 | Label it "No preset (new)". |
| ClipLook | Internal names used as row hints: "hud_draw", "r_drawviewmodel", "Uses your cl_bob", "r_decals 256". | README design rules | "The game's own HUD"; "The weapon in view"; "Bobs by your own bob settings"; in the preset hint "keeps more decals on walls". Commands-box hints may keep command names. |
| ClipLook | Side note: "Show the gun" sits under HUD here and VIEW in the game; "Overview map icons" sits under Players here and HUD in the game. "Debug, window and capture settings stay out: Studio sets those itself" also skips the demo-playback rows, which stay in the in-game tab. | `goldsrc-hooks/src/settings_tab.rs:233-244,257-261,357-390`; review.md #215, #217-#219 | Note: "Same headings as the in-game Settings tab. Some rows are named or placed differently until #899. Debug, window and demo-playback settings stay in the game." |
| ClipLook | "At each clip" hint: "Changing it mid-batch means later clips record differently" isn't the rule. The warning is about an After command with no Before for the same setting. | `studio/src/strings.js:1790-1792`; review.md #108 | "An After with no matching Before changes the value for the rest of the batch: the first clip records at one value, every later clip at another." |
| ClipsStates | The chip for decision 1 sits only on the Tick bar. The header's "Capture 45 ticked" and the menu line "the ticked clips, or every Pending one" depend on it. | review.md decision 1; ReviewCalls "How undecided items look" | Add "Undecided · Chris's call 1" beside both. |
| ClipsStates | "Why Capture is greyed" lacks G2's other warning-only cases and the one OBS case that does grey Capture. | review.md G2, G7; `studio/src/take_cleanup.js:38-40` | Add amber rows "Live, with a warning: some listed folders can't be used" and "Live, with a warning: short of room, take cleanup will make it". Add a red row "No OBS path · Add it in Settings". |
| ClipsStates, StudioClips | Two different Capture menus. StudioClips: Capture and render · Capture · Render. ClipsStates: Capture · Capture and render · Render each clip · Watch the game. | StudioClips `runItems`; ClipsStates block "Capture menu" | Use one menu on both: Capture and render · Capture · Render, then Render each clip as it is captured, then Watch the game while it records with chip 10. Add to the Render each clip line: "only with Is deleted when space runs low" (review.md #121). |
| NoProject | Kicker "after closing a project", and the app has no Close. The "Top bar" card describes a bar no page has: the name is in the sidebar's project card. | `studio/index.html:28-43`; `studio/src/project_session.js:363-433` | Kicker "First launch, or after New". Rename the block "Project card in the sidebar". |
| PopClosing | Case buttons are wrong or missing. Running: no buttons drawn. Not started: "Cancel" beside "Cancel batch and close", which reads as cancelling the batch. | `studio/src/strings.js:1707-1708,1715` | Case 2: "Close DoD Studio · Keep Studio open". Case 3: "Cancel batch and close · Keep Studio open". |
| PopMoment | Scene reference chip "Export camera for Cinema 4D" is a different feature from #1330, which exports the map. | `gh issue view 1330`: "Export maps for Cinema 4D" | "Export the map for Cinema 4D". |
| PopMoment, StudioClips | "Kinds show as a chip in Clips, so a blooper reel is one filter away", and StudioClips has no kind chip or filter. | StudioClips `filters` | Add a Kind chip to the row and a Kind filter, or drop the sentence. |
| PopMovieCopy | Stop has no confirm drawn, and it deletes what's been copied. | `studio/src/strings.js:3522-3527` | Progress row: "Copying 1,204 of 3,880 files · Stop (asks first: the part-made copy is deleted, your dod folder stays)". |
| PopProjectOpen | "3 demos have moved" over 2 found rows and one "still looking…". The search ends before this prompt opens, and the prompt lists only demos with a match. | `studio/src/project_demos.js:48,69-90` | Heading "2 demos have moved". Remove the third row. Draw "Looking for 1 missing demo" as a progress line over the Demos list, before the prompt (G3). |
| PopProjectOpen | The rescan block says "4 were scanned before marks by kills", which isn't the reason. | `studio/src/strings.js:1999-2004` | "4 demos were scanned with an older DoD Studio · about 2 min · marks, notes, status and kill ranges carry over". |
| PopStartChecks | The no-camera row offers "Skip it", and the app's choices are Start anyway or Cancel. | `studio/src/camera_data.js:28-32`; `studio/src/strings.js:2570` | Change the row's fix to "Start anyway". |
| PopSwitcher | "Save as…" isn't in the app or in review.md #1. | `studio/index.html:28-43`; `studio/src/project_file_type.js:16` (first save only) | Drop it, or label it "Proposed". |
| PopSwitcher, all sidebar pages | The kicker says clicking the project name opens the switcher, but that card links to ProjectSettings on every page. | Sidebar `<a href="ProjectSettings.dc.html">` on each page | Point the card at PopSwitcher.dc.html. The switcher already has Project settings. |
| PopTeams | The table has no Demos or Played as columns, and the note invents a per-player, per-demo view. "Read their teams again" is always shown, though review.md folds it into the rescan offer. | `studio/src/strings.js:1351-1361`; review.md #39 | Columns Tag · Demos · Played as · Name · Same team as, with Played as like "3 Allied, 2 Axis". Delete the note. Remove the button, or show it only with "2 demos were scanned before teams were read". |
| ReviewCalls | "Where it is drawn" names boards that don't exist or are incomplete: "Tools · more states" and "Settings · more states" (the boards are "Tools · progress states" and "Settings · failure states"). Call 9 omits Settings (this PC), call 2 omits Demos · more states and Project settings, and call 8's "(unchanged)" hides that the page had no chip. | `project/canvas.json` board titles | Use the real board names and list every place: 2 Pop-up · Teams, Demos · more states, Project settings; 8 Players and sprays, Tools · progress states; 9 Settings (this PC), Settings · failure states. |
| SettingsStates | Updates row says "Stable · Beta", where the app and AppSettings say Stable and Experimental. | `studio/index.html:2189-2190` | "Stable · Experimental · Check at start · Check now". |
| StudioClips | The call 3 chip reads "Undecided · call 3". | ReviewCalls "How undecided items look" | "Undecided · Chris's call 3". |
| StudioDemos | The highlight marked Skip (17:31) has the filled "+ Add to project" button. A Skip highlight can't be picked for capture. | review.md G30; `studio/src/strings.js:304`; `studio/src/detail_pane.js:513` | Grey the button with "Skipped: change the mark to add it". |
| StudioVariant | Range text says 2.0 s before and 0.6 s after; ProjectSettings says 5 s and 5 s. | StudioVariant `rangeLabel`; ProjectSettings "Clip length" | "starts 5.0 s before kill N, ends 5.0 s after kill M". |
| StudioVariant | "Command profile" and "[Your profile]" are names no other page uses, and Capture mode's "Video · Ut Video" and "Frame sequence" differ from ProjectSettings' chips. | ClipLook "Preset"; ProjectSettings "Capture with"; review.md decision 3 | Rename to "Clip look" with Same as project · Clean clips · [Your preset]. Capture mode: Same as project · Frames · Video · AGR for Blender. |
| ToolHD | "gl_max_size" is an internal name as a row label. | README design rules | "Largest texture size the game shows", keeping the value and advice. |
| ToolPlayers, ToolsStates | ToolPlayers rows show "Fallback" with no control behind it. ToolsStates lists three row names with no choices, so HLTV having no "their own" isn't shown. | review.md #130; `studio/index.html:927-950` | On ToolPlayers Options: "Spray in POV demos: their own · one picture · none"; "Spray in HLTV demos: one picture · none"; "Crosshair: the game's own · one crosshair". |
| ToolPlayers, ToolHD, `project/canvas.json` note `appRow1` | "Works through the hook DLL while capturing" and "Studio tools: only while capturing, through the hook DLL" undersell it, since the hook is also in the game for Watch in game and Review. | review.md "New contradictions" item 4 | "Applies in clips, Watch in game and Review." Note: "Studio tools: applied by the game Studio starts". |
| WebAnalyzer, WebHome | Kill map tab and view, "Copy link to stats", and "a kill map" on WebHome's Analyzer card are drawn as live. The site has 8 tabs and neither feature. | review.md G32; `web-analyzer/www/index.html:33-40`; no match in `web-analyzer/www` | Chip "Planned" on the Kill map tab and Copy link. WebHome: drop "and a kill map", or add "(planned)". |
| WebHome, WebStudio, WebAuditor, WebOverviews | These pages are planned (#603), but only one footer line on WebHome says so. WebHome shows "Works here" and "Chrome or Edge". WebStudio shows "Works now" and "Marks you make here stay in this browser", though the site has no Studio page, project file or marks. | review.md contradiction 17, G32; `web-analyzer/www/index.html` | Add a "Planned (#603)" chip to each of these page headers. WebHome cards: "Planned". WebStudio: replace "Works now" with "Planned". |

## Not drawn anywhere

| Item (row, gap or issue) | Where it should go |
|---|---|
| #12 Update available / Restart to update chip | Foot of the sidebar on every page (the canvas has no top bar), hidden with no update |
| G11 Help item (PopHelp's kicker promises it) | Foot of the sidebar on every page, above Settings |
| #22, the pop-up behind "Show which" | Pop-up listing every unreadable file and its reason |
| #69 OBS drop-rate warning | Yellow line on StudioCapturing (OBS only); per-clip totals on StudioBatchDone |
| #88 Test connection button | AppSettings OBS card (only named in a hint) |
| #90 OBS quality chips | AppSettings OBS card: Small · Standard · High · Max, with OBS's own under Custom |
| G7, the OBS card buttons | AppSettings OBS card: Test OBS capture · Find my highest steady FPS, opening PopObsTest |
| #95 Clip length line | ProjectSettings, under the two fields: "5 s, first kill ... last kill, 5 s" |
| #123 Clip-name placeholder chips | ProjectSettings, under "Name clips like" |
| #150 Default links | ToolRename beside each template; ProjectSettings beside "Name clips like" |
| #1085 Folder under each drive | ProjectSettings: "Folder under each drive: [name]" |
| #107 Save changes / Rename / Delete on a preset | ClipLook: "⋯" on your own preset chip; built-ins show only Duplicate |
| #213 Run my map .cfg files | ClipLook Map section, shown when such files exist |
| #214 HD textures Off | ClipLook HD textures box, first choice |
| #129 Make a crosshair, Add a crosshair file | ToolPlayers |
| #1322 Recorder's own spray against the Players-table spray | ToolPlayers Options, on the POV spray row, still undecided |
| #192, #204 Row ⋯ menu (Cancel, Render again, Remove, Move take to Recycle Bin, Open folder) | StudioClips row "⋯", drawn open once |
| #193 Select unrendered, Select finished with age list, Clear Queue and its confirm | Tick bar on Clips |
| G21 "Finished, older than a week…" and the measuring and recycling progress | Tick bar on Clips |
| #198 Add nameplate script to VEGAS | StudioClips Export menu, fourth item, with a note that it needs VEGAS |
| #1264, PR #1303 Export a whole project, and deleted-take videos | StudioClips Export menu: reword "Rendered clips in order"; deleted-take videos in the expanded row |
| #1265 Premiere and Resolve exports | StudioClips Export menu rows |
| #1039, #1051 Motion blur and slow-motion versions | ProjectSettings "Finished clips", as extra versions per clip |
| G9, the status line and the stop pop-up | "Waiting for renders to free room" on StudioCapturing; pop-up when the batch stops early |
| G10 Search, Recorded by and Demo filters on Clips (named only in a block subtitle) | Filter row above the Clips list |
| G19 Find progress with Cancel; #876 status per map | ToolSplitMaps: progress line and a status column |
| G20 Confirms before Rename and before Recycle | Small pop-ups for ToolRename and ToolAuditor |
| #138 Keep copies already in a folder… | ToolAuditor Found card |
| #139 Copy path, Open folder | ToolAuditor row "⋯" |
| #983 Backup to a checked archive | ToolBackups, beside the folder targets |
| #153 Search, Type chips, and one Filter chip | ToolAnalyzer demo list |
| #159 Chat filters (All, Team, alive or dead, team picker, System) | ToolAnalyzer Chat tab, drawn once |
| #160 Copy SteamID, copy show-only command | ToolAnalyzer Players tab |
| #167 Use my own Python or upscaler | ToolHD Set up pop-up |
| #168 Maps to build: All · Pick… | ToolHD Build tab, drawn once |
| #169 Style chips, 1024 · 2048 · 4096, file types under Custom | ToolHD Build tab |
| #174, #175, #177 Area · Piece · Hide · Label mode chips | ToolOverviews, above the map |
| #176 Rename flags | ToolOverviews, names list under Show |
| #180 TGA · BMP | ToolOverviews "Save to" |
| #183, #581 Undo, colour chips, Title card box (title, credit, flag icons), room for later title options | ToolOverviews |
| G24 Breakable floors, steep slopes, spawn protection with colours behind Look…, spawn names, flag names | Show box on ToolOverviews and WebOverviews |
| G25 Render greyed with no folder for finished clips | ClipsStates, with "Add a folder for finished clips in Settings" |
| G31 Quality chips (Quick 720p, Final 1080p) and Refresh | ToolBlender Render step |
| G32, #208 Open several demos, status per demo, Download all (JSON), the "None of those files are .dem demos" error | WebAnalyzer |
| G37 Update pop-up with notes and download bar | New small pop-up, from Check now and the update chip |
| #896, #1325 Explain a good capture | Help for Picture, Clip length and Setup (ClipLook's preview covers only part) |
| #1110, #621, PR #622 Room for more filters and tags | DemosStates filter row |
| Section 4: what Maybe or Skip does to a clip that already has a take | StudioClips row state, or a pop-up |
| Section 4: what can be edited during a batch, and what a change does to it | StudioCapturing note |
| Section 4: what an old project gets when opened, where old render presets and players go, and what a capture uses with no project | PopNewProject or PopProjectOpen; NoProject |
| Only if built: #585 Rough Cut, #429 nameplate burn-in, #668 HD folder on another drive, #1045 main-menu background | Sidebar; ToolPlayers; AppSettings Where files go; AppSettings Game |
| On hold for the Auditor review: #872 Consolidate demos | ToolAuditor |

## Drawn only on a state sheet

#6, #8, #19, #20, #21, #22 (end-of-scan line), #24-#28, #30 and #38 (the "⋯" menus), #32, #34, #40-#43 (row chips), #49, #53, #54, #55, #57, #64, #78, #80, #121, #130 (labels only), #136, #137, #140, #151, #155, #156, #187, #189, #195, #202 (row picture), G2, G5, G6, G8 (the setting row), G9 (menu text only), G10 (ticks and bar), G13, G15 (Clips row), G16, G17, G18, G19 (chips), G20 (chip), G22, G23, G27, G28, G29, G30, G31 (partly), G34, G35, G36, G37 (Check at start), calls 1, 5, 6 and 10, call 2's Demos link, and the Tools progress states (HD build and download, Cache this folder, Blender step).

## Agents' findings I rejected

- StudioBatchDone, remove "Render failed" from Last batch: review.md asks for "Last batch, with rendered, failed and cut-short takes".
- StudioBatchDone, 7 of 9 against 6 rows: example data, and 9 minus 7 matches "the 2".
- PopSwitcher "Close" as closing a project: it's the pop-up's dismiss button, as on PopHelp and PopObsTest. Only NoProject's kicker needs changing.
- StudioDemos, no Teams link: drawn with its chip on DemosStates.
- StudioCapturing and StudioClips, call 10 not drawn: drawn with its chip on ClipsStates. "You can keep using your PC" matches `studio/src/strings.js:578`.
- ToolAnalyzer, call 5 not drawn: drawn with its chip on ToolsStates.
- ToolHD, #78 not drawn: drawn on ToolsStates.
- AppSettings "Not drawn: #6, #8, G5, G6, G17": all on SettingsStates. ToolPlayers "Not drawn: #128, G18": on PopSmall and ToolsStates.
- PopProjectOpen, "Not now" implies a later prompt: one does come, because the search runs again at the next open and at Capture (`studio/src/project_demos.js:31,134`). The label is only plainer.
- PopRecovery, kicker "after a crash": review.md files this prompt under "Crash recovery", so it's a wording nit.
- PopNewProject, unmarked proposal: review.md asks for this page and its subtitle cites #1085.
- PopToasts, review toast wording: S does stop a review, and the count wording is a nit.
- PopToasts, "New" chips on two toasts: both are the wording review.md #139 and #101 recommend.
- StudioVariant, ghost row "FOV 100": background example data.
- ToolSplitMaps, "Split 3" with 4 ticked: example data.
- ToolOverviews and WebOverviews, "needs the hook DLL": true of the 4096×3072 copy. Only the mechanism name is in question, and it's rare text.
- WebHome and WebStudio as must-fix: moved to should fix. Contradiction 17 is a deliberate proposal, and the planned state only needs marking.
- ToolsStates chip as a separate must-fix: merged with ToolPlayers' missing chip.
- AppSetup "five steps against six": folders merged on purpose, not a mistake.

## Summary claims that weren't true

- **"All 10 decisions are marked wherever drawn": false.** Call 8 has no chip on ToolPlayers, and on ToolsStates its chip lacks "Undecided". Call 9 has no chip on AppSettings. Call 2 has none in AppSettings' side note. Call 1 has none on ClipsStates' header button or Capture menu line.
- **"Clip look matches Clean clips": false.** "Overview map icons" and "Over each player" are wrong, and the hint's "sets every switch on this page" is false. The other rows Clean clips sets match.
- True: the "same names" claim was softened (now "Same headings", though two rows still sit under different headings). Project settings has a Teams card (its call 2 mark is plain subtitle text, not a chip). The Clips footer changed. Overview map icons says HLTV only.
