# Valve's Linux engine: names for `hw.dll`

`hw.dll` carries no symbols, and the survey (`goldsrc_hw_dll_survey.md`) had to name its functions one at a time. Valve's Linux builds of the engine are unstripped: every function by name, and full DWARF. This page says which Linux files exist and where, how to fetch the ones the game installs lack, and how far their names carry over to `hw.dll`, `DemoPlayer.dll` and the rest (#1133). DoD's own `client.so` and `dod.so` have their own page, `goldsrc_linux_debug_symbols.md` (#1127).

## 1. What exists, and where

Measured 2026-11-11 with `goldsrc-hooks/tools/elf_symbols.py info`.

**On the movie installs** (read-only; nothing fetched): `dod\cl_dlls\client.so` and `dod\dlls\dod.so` on both (and their `dod_movies` copies), plus Valve's `valve\cl_dlls\client.so`, `valve\dlls\hl.so` and `valve\dlls\director.so` on POST. All carry `.symtab` and DWARF. No engine library is on either install.

**In the Half-Life Dedicated Server (app 90), which SteamCMD downloads anonymously.** Two branches, one per engine build:

| File | `public` (25th Anniversary) | `steam_legacy` (pre-Anniversary) | Windows counterpart |
|---|---|---|---|
| `sw.so` | 6,056 functions, DWARF | not shipped | `hw.dll` (client engine, software renderer instead of GL) |
| `engine_i486.so` | 2,080 functions, DWARF | 2,029 functions, DWARF | `hw.dll` / `swds.dll` (server half) |
| `demoplayer.so` | 389 functions, DWARF | 403 functions, DWARF | `DemoPlayer.dll` |
| `core.so`, `proxy.so` | DWARF | DWARF | `Core.dll`, `proxy.dll` (HLTV) |
| `filesystem_stdio.so`, `vgui.so`, `vgui2.so`, `libtier0.so`, `libvstdlib.so`, `voice_*.so` | DWARF | DWARF | the DLLs of the same names |
| `valve/…/hl.so`, `client.so`, `director.so`; `dod/…/client.so`, `dod.so`; `cstrike/…` | DWARF | DWARF | the mods' DLLs |
| `libsteam.so`, `libsteam_api.so`, `crashhandler.so` | `.symtab` only | `.symtab` only | |
| `libsteamwebrtc.so` | stripped | stripped | |

Compilers: the Anniversary files are GCC 5.4.1 (SteamRT), the legacy ones GCC 4.6.1. DoD's `client.so`/`dod.so` are the same bytes on both branches and on the installs.

**`sw.so` is the find.** It is the whole client engine (331 `CL_*`, 291 `R_*`, 108 `Host_*`, `V_*`, `SCR_*`, `Key_*`, `S_*`, the demo recorder and player, the svc handlers), built from the same source as `hw.dll` with the software renderer in place of GL. Only the Anniversary branch ships it.

**Not reachable anonymously:** `hw.so`, `GameUI.so` and the Linux client's own `DemoPlayer.so` live in Half-Life's (app 70) Linux depot 8, which DoD (app 30) also uses. SteamCMD's anonymous login answers `ERROR! Failed to install app '70' (No subscription)` (and the same for app 30), and `download_depot 70 8` answers `Depot download failed : missing license for depot (No subscription)`. Fetching them needs an account that owns Half-Life or DoD. That has since been done through the Steam client's own console (section 6): both builds of `hw.so` and `gameui.so` are fetched and measured.

**With an account (Half-Life app 70, Linux depot 8):**

| File | `public` (25th Anniversary), manifest 1360709870205537514 | `steam_legacy` (pre-Anniversary), manifest 8088891443299905514 | Windows counterpart |
|---|---|---|---|
| `hw.so` | 6,408 functions, DWARF, `Oct  8 2024` | 6,002 functions, DWARF, `Aug  3 2020`, `Aug 19 2020` | `hw.dll` (the GL client engine) |
| `valve/cl_dlls/gameui.so` | 3,778 functions, DWARF, `Oct  8 2024` | 3,755 functions, DWARF, `Aug  3 2020` | `valve\cl_dlls\GameUI.dll` |
| `demoplayer.so`, `engine_i486.so`, `sw.so` (`public` only), `core.so`, `proxy.so`, `vgui2.so`, ... | as in app 90 | as in app 90 (the legacy `engine_i486.so` is byte-identical to app 90's) | |

Same compilers as the app 90 files (GCC 5.4.1 for `public`, 4.6.1 for `steam_legacy`).

### Which build each file is

The engine prints its build date from `__DATE__`, so the date strings in `.rodata` date a build. `steam_legacy`'s `engine_i486.so` has `Aug  3 2020` and `Aug 19 2020`, the same two dates as the pre-Anniversary `hw.dll`. `public`'s `engine_i486.so`, `sw.so` and `demoplayer.so` have `Oct  8 2024`; the 25th Anniversary `hw.dll` has `Oct  7 2024`, the same release built a day earlier (Steam's `public` build is 15961496, updated 2024-10-08). `app_info_print 90` describes `steam_legacy` as "Pre-25th Anniversary Build".

## 2. How to fetch them

Download SteamCMD from Valve (`https://steamcdn-a.akamaihd.net/client/installer/steamcmd.zip`, Authenticode-signed by Valve) into its own folder, outside every install and outside the repo, and run it with a script per branch. Anonymous only; this never touches the Steam client or its login:

```
@ShutdownOnFailedCommand 0
@NoPromptForPassword 1
@sSteamCmdForcePlatformType linux
force_install_dir D:\<somewhere>\app90-public
login anonymous
app_set_config 90 mod dod
app_update 90 validate
app_update 90 validate
quit
```

`steamcmd.exe +runscript <script>`. For the pre-Anniversary files use another folder and `app_update 90 -beta steam_legacy validate`. Run `app_update` more than once: app 90 is known to stop short on the first pass. Each branch is about 1.3 GB with the mods' content; the libraries are a small part of it. `force_install_dir` must come before `login`, or SteamCMD warns (it still used the folder in this run).

## 3. What they can name

| Windows file | Linux file to read | What it names |
|---|---|---|
| `hw.dll`, Anniversary | `sw.so` (`public`), then `engine_i486.so` (`public`) | the client engine: demo playback, svc handlers, `V_*`, `SCR_*`, `Key_*`, `S_*`, `Con_*`, the shared `Cbuf_*`/`Cmd_*`/`COM_*`/`MSG_*`/`Mod_*`/`PM_*`/`DELTA_*`; GameUI-side `vgui2::` classes by vtable |
| `hw.dll`, pre-Anniversary | `sw.so` (`public`, a different build, still works), then `engine_i486.so` (`steam_legacy`, the same build) | the same |
| `hw.dll`, either build | **`hw.so` of the same branch (app 70, section 6)**, then `sw.so`, then `engine_i486.so` | everything above plus the GL renderer: `GL_*`, the GL-side `R_*`, `DT_*` (detail textures), `QGL_Init`, `VID_*`, `CVideoMode_OpenGL`, `EngineSurface`, the statically linked `vgui2::` classes |
| `GameUI.dll` | `gameui.so` of the same branch (app 70) | the `vgui2::` widget classes by vtable, `CGameUI` and the dialogs |
| `DemoPlayer.dll` | `demoplayer.so` of the same branch | the `DemoPlayer` class by vtable (it has RTTI), a few more by strings |
| `client.dll` | DoD's `client.so` | already done: `name_client_dll.py`, `goldsrc_linux_debug_symbols.md` |

What the app 90 files alone could not name: GL renderer code (`GL_*`, `R_*` that differ between the GL and software renderers: `R_DecalInit`, `R_BuildLightMap` went unnamed) and GameUI's own DLL. `hw.so` and `gameui.so` cover both (section 6). What still goes unnamed: Windows-only code (`Sys_*` for Win32, the CD player) and anything the Linux build lacks or compiles differently. As with `client.so`: **names yes, numbers no**. Every offset, struct layout and signature still comes from the Windows binary; the Linux names only say what a function is.

## 4. Matching `hw.dll` to a `.so`

`goldsrc-hooks/tools/name_hw_dll.py --hw <hw.dll> --so <hw.so> [<sw.so> <engine_i486.so> ...] [--out map.tsv] [--check pre|anniversary] [--find NAME ...]` builds the map. It reuses `name_client_dll.py`'s matcher (vtables where the DLL has RTTI, rare strings and constants, functions registered beside a string such as console commands, call-graph gaps between matched calls) and adds the engine's dispatch tables: `cl_parsefuncs` (opcode, `svc_*` name, handler) is found in `hw.dll` by its strings, and pure function-pointer tables by a unique length. Two engine-specific limits: no `cl_enginefunc_t` slots (only `client.dll` calls through them), and a call-graph pair needs a function of at least 64 bytes, because MSVC inlines the small helpers (`Q_strcmp`, `VectorMA`, the `FS_*` wrappers) that GCC calls, which shifts the gaps. About 15 seconds per run.

**Results** (2026-11-11):

| `hw.dll` | `.so` | Named | Known functions: hit / wrong / unnamed |
|---|---|---|---|
| pre-Anniversary (7,268 functions found) | `sw.so` | 2,004 (43% of `.text` bytes) | 18 / 0 / 10 of 28 |
| pre-Anniversary | `engine_i486.so` (legacy) | 589 | 4 / 0 / 2 (22 absent from the server) |
| Anniversary (8,373 functions found) | `sw.so` | 2,020 | 10 / 0 / 6 of 16 |
| Anniversary | `engine_i486.so` (public) | 506 | 2 / 0 / 2 (12 absent) |

The two maps of one `hw.dll` together name 2,096 (pre-Anniversary) and 2,037 (Anniversary) functions. The survey named about 106.

**With `hw.so`** (measured 2026-11-12, same build as the DLL):

| `hw.dll` | `.so` | Named | Known functions: hit / wrong / unnamed |
|---|---|---|---|
| pre-Anniversary | `hw.so` (`steam_legacy`) | 2,662 (37% of functions, **52% of `.text` bytes**) | 21 / 0 / 7 of 28 |
| pre-Anniversary | `hw.so` + `sw.so` + `engine_i486.so` | 2,775 (**55% of bytes**) | 25 / 0 / 3 of 28 |
| Anniversary | `hw.so` (`public`) | 2,421 (29% of functions, **24% of bytes**) | 12 / 0 / 4 of 16 |
| Anniversary | `hw.so` + `sw.so` + `engine_i486.so` | 2,443 (25% of bytes) | 12 / 0 / 4 of 16 |

`hw.so` alone beats `sw.so` by 658 functions on pre-Anniversary (2,662 against 2,004) and 401 on Anniversary (2,421 against 2,020), even though `sw.so` was on the pre-Anniversary side a different build; `sw.so` and `engine_i486.so` add only 113 (pre) and 22 (Anniversary) names on top. The Anniversary `.text` percentage is low because that `hw.dll` is much bigger (8,373 functions found, against 7,268): the extra is code with no counterpart in the Linux engine.

Areas `hw.so` adds over `sw.so` and `engine_i486.so` (pre-Anniversary: 679 functions neither names, Anniversary: 406): the GL renderer (`GL_Bind`, `GL_LoadTexture2`, `GL_Upload16`, `GL_SelectTexture`, `GL_MakeAliasModelDisplayLists`, `R_RenderDlight`, `R_LightPoint`, `R_CullBox`, `R_DrawAliasModel`, `R_DrawSpriteModel`, `R_SetupFrame`, `R_SetupGL`, `R_Envmap_f`; on Anniversary also `R_StudioDrawBones`, `R_GLStudioDrawPoints`, `R_RenderFinalFog`), 18 and 15 `GL_*` names, 53 and 36 `R_*`, the detail-texture code (`DT_Initialize`, `DT_Parse`, `DT_LoadDetailMapFile`, `DT_LoadDetailTexture` pre only, `DT_SetRenderState`), `QGL_Init`, `VID_Init`, `GLimp_*`, `CVideoMode_OpenGL`/`CVideoMode_Common`, `EngineSurface`, and (pre-Anniversary) 121 `vgui2::` class functions by vtable. `R_BuildLightMap` and `R_DecalInit` are named now; `R_DecalUnlink`, `V_CheckGamma` (pre), `ClientDLL_Key_Event`, `PM_RecursiveHullCheck`/`Mod_LoadBrushModel` (Anniversary) are not.

**Wrong names.** Against the 28 + 16 hand-located functions: **0 wrong** in every combination. (`PM_PlayerTraceEx` is `_PM_PlayerTrace` on Linux, and `CL_CheckGameDirectory` is inlined into `CL_BuildMapCycleListHints`, whose body holds its strings; the tool maps both and strips GCC's `.part.N` clone suffixes before comparing.) The wider cross-check is the same `hw.dll` named from another `.so`:

- Pre-Anniversary: `hw.so` against `sw.so` shares 1,898 functions and differs on 12; against `engine_i486.so` 576 shared, 6 differ. Anniversary: 2,012 shared, 1 differs (`sw.so`); 490 shared, 12 differ (`engine_i486.so`). Of those 31 rvas, `hw.so` is right on most (checked by a string in the function, its callers, or the other file naming it for another build: `Host_ShutdownServer` has "Server shutdown", `Netchan_Clear` its own message, `COM_HasFileExtension` is called with demo names, `R_SetupGL` and `GL_EndRendering` exist only in `hw.so`). **`hw.so`'s own names are wrong in at least five**: `Sys_Printf` at pre `+0x294b0` (it is `Q_strcat`: callers are `Host_Say`, `Cmd_Alias_f`...), `Mod_LoadModel` at pre `+0xaa790`, `vgui2::MessageBox::MessageBox` at pre `+0x107160` (a helper called from `Cmd_AddCommandWithFlags` and `CRC_MapFile`), and on Anniversary `ModelFrameCount` at `+0x1bb8c0` (it holds `"qconsole.log"`, so it is the console print function) and `CBaseUI::Shutdown` at `+0x22c4c0` (its strings are `cstrike`/`czero`: `GetCurrentSteamAppName`). Four are `callgraph` names and one a `vtable` name. Another 10 or so small ones (`Q_memset` against `Q_strcmp`, `SystemWrapper::FindModule` against `GetModule`, `COM_Munge` against `Q_memcpy`) are tiny functions the evidence cannot settle. So expect about 5 to 15 bad names in 2,400 to 2,700, nearly all `callgraph` or a small vtable slot.
- Self-naming messages: 98 agree and 38 differ on pre-Anniversary, 91 and 29 on Anniversary. The differing ones are the usual old or inlined names (`MOD_LoadBmodel` in each `Mod_Load*`, `HPAK_List` in the `HPAK_*` helpers, `Host_Changelevel2` in `Host_Changelevel2_f`, `S_Startup` in `S_Init`), and the same functions are named identically by `sw.so` and `engine_i486.so` on the spot checks (`DELTA_ParseType`, `Z_Print`, `NET_ParseServerList`, `Draw_CacheLoadFromCustom`).
- The tool's `--so` takes several files and reports the combined map (first listed wins); it flags 19 (pre) and 13 (Anniversary) rvas two files name differently, after ignoring `.part.N` clone suffixes (47 and 18 before).

**`GameUI.dll`** (`--hw <GameUI.dll> --so gameui.so`, no tool change needed): pre-Anniversary names 1,967 of 3,997 functions (**57% of `.text` bytes**: 1,542 by vtable, 277 features, 136 call graph, 11 callbacks), Anniversary 1,732 of 4,036 (**67%**: 1,402 vtable, 210 features, 103 call graph). GameUI's `vgui2::` widgets (`Panel`, `Frame`, `TextEntry`, `ListPanel`, `PropertySheet`...) are statically linked and have RTTI, so almost all of it is vtable matching. Checked against the slot numbers `studio_panel.rs` and the verify scripts already use (24 slots over `Panel`, `Frame`, `Button`, `ListPanel`, `PropertySheet`, `TextEntry`, `Label`): 19 pre / 17 Anniversary agree with the name each constant carries, 4 are unnamed (`TextEntry::GetText`, `Label::SetText`/`GetText`, `ListPanel::GetItem`: inlined or folded), and the rest are three folded duplicates (Anniversary slot 96 `Panel::OnMousePressed` is an empty body the linker merged with `Panel::OnChildAdded`; `PropertySheet::GetActivePage` with `SectionedListPanel::GetHighestItemID`, a field getter) and one swap worth checking: **`gameui.so` has `OnKeyCodePressed` in slot 100 and `OnKeyCodeTyped` in slot 101 (both builds), the other way round from `PANEL_SLOT_ON_KEY_CODE_TYPED = 100` / `..._PRESSED = 101` in `studio_panel.rs`.** The slots are right (they are what the hooks measured); only the constants' names (and the comments over them) are likely swapped. For the Review form (#1311): `vgui2::TextEntry::TextEntry` (pre `+0x5a120`, Anniversary `+0x60e70`), `TextEntry::OnKeyTyped` (`+0x5c660`, `+0x64a40`) and `Panel::OnMessage` (`+0x47520`, `+0x4a7a0`) are named, as are `Panel::SetVisible`, `Frame::Activate`/`OnCommand`, and the whole `ListPanel`/`PropertySheet` set the in-game panel drives. GameUI's own classes come with the same method (650 `C*::` names pre-Anniversary, e.g. `CBasePanel`, `CCreateMultiplayerGameDialog`).

**How right are they?** Three checks, all on the shipped DLLs:

- **Known functions.** `KNOWN` in `name_hw_dll.py` holds the functions this project located by hand (survey §2, §3.1, §7, §8, §14.9 and the `goldsrc-hooks` modules' tables: `Cbuf_Init`, `CL_ParseServerMessage`, `CL_ParsePacketEntities`, `playdemo`/`viewdemo`/`timedemo`, `CL_HudMessage`, `SCR_CenterPrint`, `Con_ToggleConsole_f`, `Mod_LoadEntities`, `SV_RecursiveHullCheck`, ...). No map names any of them wrongly.
- **Two `.so` files, one `hw.dll`.** `sw.so` and `engine_i486.so` are different builds by different compilers, so where both name a function they agree only if both are right. Pre-Anniversary: 474 of 482 shared names agree; Anniversary: 476 of 483. Every disagreement involves a call-graph pair (`Host_Frame`, `Cbuf_Execute`, `Q_atoi`, `NET_Config`, ...).
- **Self-naming messages.** GoldSrc's errors start with the function's name. Of the named functions whose own message names a function, about 3 in 4 agree outright; the rest are nearly all messages that name a function inlined into this one or an old name (`Mod_LoadFaces` printing `MOD_LoadBmodel`, `Cmd_AddCommandWithFlags` printing `Cmd_AddCommand`), not wrong names.

So: names found by `table`, `callback`, `vtable` and `features` are reliable; a `callgraph` name is a lead to check (strings, constants, its callers), as section 5 does.

**Not found and why:** `PM_RecursiveHullCheck` (GCC split it into `PM_RecursiveHullCheck.part.0`), `ClientDLL_Key_Event` and `CL_CheckGameDirectory` (small, no strings, inlined differently), and the GL-side `R_*` above.

### Cross-checks this pass made by hand (pre-Anniversary `hw.dll`)

Functions the project had not named, checked against disassembly:

| Function | `hw.dll` | Evidence |
|---|---|---|
| `SZ_Alloc` | `+0x2ac10` | the known `Cbuf_Init` calls it twice, as the source does |
| `Cbuf_AddText` | `+0x272e0` | appends to `cmd_text` (`Cbuf_Init` names both buffers) |
| `Cbuf_AddFilteredText` | `+0x27350` | appends to `filteredcmd_text`; `sw.so` has the pair side by side |
| `Cbuf_Execute` | `+0x27570` | runs `cmd_text`, then `filteredcmd_text` |
| `Cmd_AddCommand` | `+0x281a0` | calls `+0x28090` (`Cmd_AddCommandWithFlags`, its messages) with flags 0 |
| `Cmd_ExecuteString` | `+0x286a0` | both maps agree; a wrapper that calls `+0x284c0` with an extra argument of 1 (the body is not checked further) |
| `Host_EndGame` | `+0x55ea0` | `"Host_EndGame: %s\n"`, `"cd stop\n"`, the same as `sw.so` |
| `Host_Error` | `+0x55f60` | `"Host_Error: recursively entered"` |
| `Con_Printf` | `+0x2cb70` | `"qconsole.log"` |
| `CL_Record_f` | `+0x1010d` | `"Already recording."`, `"Can't record during demo playback."` |
| `CL_StopPlayback` | `+0xfd5d` | `"cd stop\n"`, as `sw.so`'s; a call-graph pair, so the weakest here and still to check |

One correction came out of it: survey §13.3 called the demo reader's `ConsoleCommand` call `Cbuf_AddText`. It goes to `+0x27350`, `Cbuf_AddFilteredText`, which matches what `filtered_commands.rs` found from the other side.

### Recipe: find a named Linux function in `hw.dll`

1. `python goldsrc-hooks/tools/elf_symbols.py syms <sw.so> --grep "^CL_.*Demo" --demangle` to find the name.
2. `python goldsrc-hooks/tools/name_hw_dll.py --hw <hw.dll> --so <sw.so> --find <Name>` for the address the map gives and how it was found.
3. Not named? Pick a named caller or callee (`--out` the map, look up the neighbours) and follow the call, or search `hw.dll` for one of the function's strings (`survey_hw_dll.py`'s `Image.xrefs_to`).
4. Before a hook relies on it: check the address against disassembly, then write its signature into a `verify_*_offsets.py` as for every other hook. The map never goes into a hook directly.

## 5. Legal posture

Valve's binaries are Valve's: they are fetched by each developer with SteamCMD into a folder outside every install and outside the repository, and they are never committed, copied into the repo or redistributed. What this project keeps is its own: function names used as identifiers, our own notes and the addresses in `hw.dll` they lead to, as with DoD's `client.so` (`goldsrc_linux_debug_symbols.md`) and the rest of the licensing posture (GPL and HLAE sources: ideas and facts, never code). The name maps are not committed either; the tools regenerate them.

## 6. Fetching `hw.so` and `gameui.so` (done 2026-11-12)

Half-Life's Linux depot (app 70, depot 8) holds `hw.so`, `valve/cl_dlls/gameui.so` and the rest of the Linux client. It is licensed to anyone who owns Half-Life or Day of Defeat, so anonymous SteamCMD cannot fetch it (section 1). **What worked: the Steam client's own console**, signed in as the {$T} Brain account (never the main one). It needs no password given to any tool and no SteamCMD login. Open the console (`steam://open/console`) and enter, one build at a time:

```
download_depot 70 8
download_depot 70 8 8088891443299905514
```

The first fetches the current `public` build (25th Anniversary, manifest 1360709870205537514); the second the `steam_legacy` build (pre-Anniversary). The console says where the files land; move or copy each build to its own folder (here `D:\dodstudio-tests\linux-depot\app70-depot8-public\` and `...-steam_legacy\`), and never into an install or the repo. The manifest ids are the `public` and `steam_legacy` entries of `app_info_print 70`.

**Two traps, both hit.**

1. **Windows cannot create the depot's symlink** (`libSDL2-2.0.so.0` in the legacy build) without administrator rights, and the download stops there with an error. The files before it had already been written, so the fetch is usable; the symlink itself is not needed.
2. **Both manifests download into the same folder, and the second overwrites the first in place.** A file the new build shortens keeps the old build's tail: the legacy `hw.so`, `gameui.so` and the other libraries were each followed by the rest of the `public` file. Fetch each manifest into an empty folder, or cut each `.so` to the size its ELF headers give (the end of the section header table, or the end of the last section, whichever is larger). The legacy files here were cut that way (`README.txt` in that folder says so), and all of them parse cleanly with `elf_symbols.py info` (`.symtab` and DWARF present); `engine_i486.so` came out byte-identical to the anonymous app 90 `steam_legacy` one, and the `__DATE__` strings (`Aug  3 2020`, `Aug 19 2020`) match the pre-Anniversary `hw.dll`. `sw.so` is not in `steam_legacy`'s depot and was removed from the legacy folder.

**Result (section 4):** `hw.so` names 2,662 functions of the pre-Anniversary `hw.dll` (52% of `.text` bytes) and 2,421 of the Anniversary one (24%); together with `sw.so` and `engine_i486.so` 55% on pre-Anniversary. It adds the GL renderer, the detail-texture code and the video layer. `gameui.so` names 57% (pre) and 67% (Anniversary) of `GameUI.dll`'s bytes, including the `vgui2::TextEntry` and `Panel::OnMessage` the Review form (#1311) uses. No hand-located function is named wrongly; the names themselves are wrong in a handful of cases (section 4).

**Left to do.** Hook modules still take their addresses from the verify scripts, as before; this only gives their functions names. The wrong-name leads in section 4 (five `hw.so` names, the `OnKeyCodePressed`/`Typed` constant names in `studio_panel.rs`) are unchecked in-game. Section 5 applies: the files stay outside the repo.
