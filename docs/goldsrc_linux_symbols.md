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

**Not reachable anonymously:** `hw.so`, `GameUI.so` and the Linux client's own `DemoPlayer.so` live in Half-Life's (app 70) Linux depot 8, which DoD (app 30) also uses. SteamCMD's anonymous login answers `ERROR! Failed to install app '70' (No subscription)` (and the same for app 30), and `download_depot 70 8` answers `Depot download failed : missing license for depot (No subscription)`. Fetching them needs an account that owns Half-Life or DoD (section 6).

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
| `DemoPlayer.dll` | `demoplayer.so` of the same branch | the `DemoPlayer` class by vtable (it has RTTI), a few more by strings |
| `client.dll` | DoD's `client.so` | already done: `name_client_dll.py`, `goldsrc_linux_debug_symbols.md` |

What none of them names: GL renderer code (`GL_*`, `R_*` that differ between the GL and software renderers: `R_DecalInit`, `R_DecalUnlink`, `R_BuildLightMap` went unnamed), Windows-only code (`Sys_*` for Win32, `VID_*`, the CD player), and GameUI's own DLL, which only `GameUI.so` would cover. As with `client.so`: **names yes, numbers no**. Every offset, struct layout and signature still comes from the Windows binary; the Linux names only say what a function is.

## 4. Matching `hw.dll` to a `.so`

`goldsrc-hooks/tools/name_hw_dll.py --hw <hw.dll> --so <sw.so|engine_i486.so> [--out map.tsv] [--check pre|anniversary] [--find NAME ...]` builds the map. It reuses `name_client_dll.py`'s matcher (vtables where the DLL has RTTI, rare strings and constants, functions registered beside a string such as console commands, call-graph gaps between matched calls) and adds the engine's dispatch tables: `cl_parsefuncs` (opcode, `svc_*` name, handler) is found in `hw.dll` by its strings, and pure function-pointer tables by a unique length. Two engine-specific limits: no `cl_enginefunc_t` slots (only `client.dll` calls through them), and a call-graph pair needs a function of at least 64 bytes, because MSVC inlines the small helpers (`Q_strcmp`, `VectorMA`, the `FS_*` wrappers) that GCC calls, which shifts the gaps. About 15 seconds per run.

**Results** (2026-11-11):

| `hw.dll` | `.so` | Named | Known functions: hit / wrong / unnamed |
|---|---|---|---|
| pre-Anniversary (7,268 functions found) | `sw.so` | 2,004 (43% of `.text` bytes) | 18 / 0 / 10 of 28 |
| pre-Anniversary | `engine_i486.so` (legacy) | 589 | 4 / 0 / 2 (22 absent from the server) |
| Anniversary (8,373 functions found) | `sw.so` | 2,020 | 10 / 0 / 6 of 16 |
| Anniversary | `engine_i486.so` (public) | 506 | 2 / 0 / 2 (12 absent) |

The two maps of one `hw.dll` together name 2,096 (pre-Anniversary) and 2,037 (Anniversary) functions. The survey named about 106.

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

## 6. Fetching `hw.so` with an account (not done)

`hw.so` would add the GL renderer's names (`GL_*`, the GL-side `R_*`), and `GameUI.so` the GameUI module's. Both are in app 70's Linux depot 8 (manifest per branch: `public`, `steam_legacy`), licensed to anyone who owns Half-Life or Day of Defeat. A fetch would be `login <account>`, then `download_depot 70 8` (add the `steam_legacy` manifest id from `app_info_print 70` for the pre-Anniversary files), with `@sSteamCmdForcePlatformType linux`. It needs an account's password, and Steam Guard may ask for a code; under this project's rules it would be the {$T} Brain account, never the main one. It has not been done: the user decides.
