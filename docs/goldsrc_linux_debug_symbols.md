# DoD's Linux builds: Valve's own names for client.dll and the demo's messages

Both movie installs ship DoD's Linux builds next to the Windows DLLs, and Valve left them unstripped with full DWARF debug info. They give the real names of the functions, classes, members and user messages that every other doc here had to find by disassembly. This page says what they hold, what may be taken from them, and what the three tools in `goldsrc-hooks/tools/` that read them do and how far to trust them (#1127).

## 1. What the files hold

| File | Size | Symbols | Debug info |
|---|---|---|---|
| `dod/cl_dlls/client.so` | 8.4 MB | 4,636 functions in `.symtab` (4,418 distinct addresses), ~1,400 objects | 154 compile units, every class's members in order, local variable names |
| `dod/dlls/dod.so` | 7.0 MB | ~5,400 functions and objects, every `gmsg*` global | 103 compile units |

- Built by GNU C++ 4.6.1 on Valve's buildbot from the same source as `client.dll` and `dod.dll`. The PRE and POST installs carry byte-identical copies (and byte-identical `client.dll`s), so one reading serves both engine builds.
- Both are linked without `-fPIC`: every absolute address in their code is patched through a relocation (`R_386_32`, `R_386_RELATIVE`), and calls to global functions go through `R_386_PC32` relocations. That is why a plain `E8` scan of `dod.so` finds no callers; the tools read the relocation table instead.
- `dod.so` is the server: it is what writes every user message into a demo, so it says exactly what each message carries (section 4).

## 2. Names carry over; offsets do not

Same source, different compilers. Names, the order of class members and the order of virtual functions are the same in both builds. Offsets are not:

- MSVC aligns a `double` member to 8 where GCC uses 4, and some member classes differ in size. In `CHud`, every field after `double m_flTimeDelta` sits 4 bytes later in `client.dll` (`m_iHideHUDDisplay`: DWARF `+0x54`, `client.dll` `+0x58`), and by the cached-cvar block the gap is −0x3ec.
- GCC gives a virtual destructor two vtable slots (complete and deleting); MSVC gives it one. Otherwise the vtables line up slot for slot (section 3).

So the rule is the same as for the reconstructed dod13-client source, but with Valve's real names: **take names and member order from the `.so`, take every number from `client.dll`**, and check each against the Windows binary before a hook relies on it (`goldsrc-hooks/tools/verify_*`).

## 3. Naming client.dll's functions

`name_client_dll.py` pairs `client.dll`'s functions with `client.so`'s and writes a map (rva, name, how it was found). It runs offline in about 10 seconds. Methods, strongest first:

| Method | Evidence | Pairs |
|---|---|---|
| export | the same exported name (`HUD_Redraw`, `F`, `weapon_*`) | 123 |
| vtable | the same slot of the same class's vftable; `client.dll`'s RTTI names its classes. Only classes whose slot counts agree once destructors are collapsed (178 of 196) | 1,857 |
| callback | a function pointer passed beside the same string (commands, user messages, events: `pfnHookEvent("events/weapons/colt.sc", EV_FireColt)`) | 220 |
| features | mutual best match on rare strings, constants and `cl_enginefunc_t` slots called (the engine table is the same struct in both builds) | 279 |
| callgraph | between two matched calls, the one to three unmatched calls in between pair up in order | 232 |

Each pair from the last three must also pass two checks: sizes within a factor of 3 (for functions over 32 bytes), and two functions that both reference strings must share one.

**Coverage** (measured on the shipped `client.dll`): 2,711 of the 4,927 functions the tool finds are named, 55% of functions and 64% of `.text`'s bytes. 61% of `client.so`'s functions found their partner. Most of what stays unnamed has no partner to find: about 490 functions at `+0x8b000`..`+0x9bc00` hold 4 names between them, and that block includes the statically linked MSVC C runtime (the Linux build imports libc instead). The rest is mostly code GCC and MSVC inlined differently, and the ten classes whose vftables differ in length between the builds (`CClientScoreBoardDialog`, `CSpectatorGUI`, `DoDViewport`, `vgui2::HTML`, `CVoiceStatus`, ...).

**Reliability.** Measured three ways:

- *Hold-out.* With the vtable method switched off, the other methods named 276 of the functions the vtables and exports name. They agreed on 273: `features` 251 of 253 (99%), `callgraph` 22 of 23 (96%). Of the three disagreements, one is an empty function MSVC kept once for two names (`PrintState` / `PrescheduleThink`), so the true error rate is lower.
- *Against names already in the repo.* 26 functions that docs or hook comments name with a `client.dll` offset (34 offsets) come out the same. Two came out different, and in both the map is right: `engine.rs` said `+0x36b32` is in `CHud::VidInit`, but that function makes exactly `CHud::Think`'s four engine calls and plays its two sounds; and `goldsrc_objective_icons.md` said `CHudSpectator::Reset` zeroes the spectator-mode global at `+0x3a15e`, but that address is in vftable slot 7, which `CHudBase`'s DWARF names `InitHUDData` (both fixed in this PR).
- *Hand-identified functions* (`survey_client_dll.py`'s `KNOWN_FUNCTIONS`): 8 of 10 named, 4 of them the same. The other 4 are our guesses next to Valve's names, below.

Treat `export`, `vtable` and `callback` as names, and `features` and `callgraph` as strong leads that a hook still checks by disassembly. MSVC folds identical functions, so a tiny function can legitimately carry two names.

### Our guesses next to Valve's names

| rva | Name used in this repo | Name in `client.so` | Method |
|---|---|---|---|
| `+0x228e0` | `CHud::OverviewMapMode` | `CHud::GetMinimapState` | callgraph |
| `+0x22a60` | `CHud::GetOverviewMapBounds` | `CHud::GetMapBounds` | features |
| `+0x22970` | `CHud::ComputeOverviewMapRects` | `CHud::InitMapBounds` | features |
| `+0x38850` | `CHudSpectator::SetMode` | `CHudSpectator::SetModes` | features |
| `+0x207b0` | `CHintPanel::SetHint` | `CHintPanel::ShowHintMessage` | features |
| `+0x3a120` | `CHudSpectator::Reset` (one doc) | `CHudSpectator::InitHUDData` | vtable |
| `+0x36b20` | `CHud::VidInit` (one comment) | `CHud::Think` | features |

The last two were wrong and are fixed. The first five are renames for whoever next touches the code that uses them: a rename touches comments, docs and the survey script together (CLAUDE.md), so it is left out of this PR. `CHud::ShouldDraw` (`+0x2ca80`) and `CObjectiveIcons::DrawDigit` (`+0x30620`) stay unnamed.

## 4. User message layouts

`user_message_layouts.py` reads each message from both ends: `writers` lists every `MESSAGE_BEGIN` ... `MESSAGE_END` in `dod.so` with its `WRITE_*` calls in order, and `readers` lists every `MsgFunc_*` in `client.so` with its `READ_*` calls. The writer side also reads `LinkUserMessages`, which gives each message's wire name and registered size (`-1` = variable length). The name on the wire is not always the global's: `gmsgPlayersInArea` registers `PlayersIn`.

Checked against the registered sizes: 97 of the 102 fixed-size writer sites with no loop or helper call add up to exactly their message's size. The five that don't are sites where the message id was loaded before a branch and the tool credited the wrong message; the layout of a message is still right at its other sites. Loops (`{...}*`) are read from backward jumps, not a control-flow graph, and a field behind an `if` is listed like any other, so read the function before relying on an optional or repeated field.

Messages the analyzer drops today, as the server writes them:

| Message | Size | Server writers | Fields |
|---|---|---|---|
| `ClientAreas` | var | `CAreaCapture::area_SendStatus`, `area_SetIndex` (also on `CObjectCapture`, `CBreakable`); `CBasePlayer::SetClientAreaIcon`, `HandleSignals`; `CDoDTeamPlay::LevelChangeResets` | `BYTE, BYTE, STRING` from the areas, `BYTE, BYTE` from the player and the round reset |
| `PlayersIn` | 4 | `CAreaCapture::SendNumPlayers` | `BYTE, BYTE, BYTE, BYTE` |
| `CurMarker` | 1 | `CBasePlayer::NextMapMarker`, `PrevMapMarker` | `BYTE` |
| `MapMarker` | — | none: registered, never sent | — |

What those fields mean, and what POV and HLTV demos carry of them, is #1132.

## 5. The tools

All three are offline Python (`pip install pyelftools pefile capstone`), default to the pre-Anniversary movies install and take `--install PATH`.

```
python goldsrc-hooks/tools/linux_so.py client|dod syms             # every function and object, readable and mangled
python goldsrc-hooks/tools/linux_so.py client|dod cus              # compile units (source files) and compiler
python goldsrc-hooks/tools/linux_so.py client|dod struct CHud ...  # member offsets, types and names (GCC's offsets)
python goldsrc-hooks/tools/linux_so.py client|dod callers NAME     # who calls a function, relocations included
python goldsrc-hooks/tools/name_client_dll.py --out names.tsv      # the client.dll name map, summary on stderr
python goldsrc-hooks/tools/name_client_dll.py --no vtable          # leave a method out to measure the others
python goldsrc-hooks/tools/user_message_layouts.py writers|readers [NAME...]
```

The map is not checked in: it is derived from Valve's binaries in ten seconds, and a copy would only go stale against the tool.

## 6. Licensing

These are Valve binaries the game ships to every player. Reading their symbol tables and debug info is the same posture as reading `client.dll`'s RTTI: observing a binary we already analyse. Names, member order and message layouts go into docs and code comments. No code is copied, and there is none to copy: the files hold machine code, symbols and type descriptions, not source.
