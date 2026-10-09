# GoldSrc `hw.dll`: what HLAE already owns, and what is left

Sibling to `docs/goldsrc_client_dll_survey.md`. That one covers DoD's client
library, where we have the module to ourselves. This one covers the engine,
where we do not.

Opened as [issue #256](https://github.com/ccoventry/dod-studio/issues/256), which
sets the order: **subtraction first.** HLAE patches `hw.dll` heavily, two hooks
over one span destroy each other, and the list of what HLAE already solves is
also a list of what is probably a duplicate rather than a gain.

**Subjects:** `hw.dll` from the *Half-Life - Pre-Anniversary for Movies* depot,
1,641,376 bytes on disk, `ImageBase 0x1d00000`, `.text` 1,159,168 bytes — **ten
times `client.dll`'s**. And `AfxHookGoldSrc.dll` from the matching HLAE install,
read but never modified.

**Method:** offline, `pefile` + `capstone`. The tables are produced by
`goldsrc-hooks/tools/survey_hw_dll.py`:

```
python goldsrc-hooks/tools/survey_hw_dll.py [keys|collide|findings]
```

> ### Status (2026-10)
>
> §1–§5 are the first pass; §7–§11 are later passes (#273, closed) that resolved
> most of what §3.3–§3.5 and §4 leave open. Where the two disagree, trust the
> later section; §11 is what is still not surveyed. §12–§13 are #300: §12
> redoes the HLAE map against HLAE 2.192.4 for **both** engine builds, and
> supersedes §1–§2's counts.
>
> | Candidate | Built? | Command / module | Issue / PR |
> | --- | --- | --- | --- |
> | raise `ex_interp`'s 100 ms ceiling (§3.2) | yes | `dodstudio_ex_interp_max`, `ex_interp.rs` | #271, PR #301 |
> | empty the decal pool (§3.4, §9) | yes | `dodstudio_clear_decals`, `decals.rs` | PR #297; #290 still open |
> | the 64-byte command limit (§3.1) | not reachable | — | #256 |
> | `CL_ParseServerMessage` / demo reader (§3.3) | no | — | — |
> | `CL_FlushEntityPacket` predicate (§3.5, §8) | located offline, no command | — | #273 |
> | map entity trim (`MAX_PACKET_ENTITIES`) | no | — | #207, open |
>
> ### Read this before §1
>
> **This is a partial survey, and says so up front.** `hw.dll` is ten times the
> size of `client.dll` and has neither RTTI nor useful exports, so none of what
> made the client survey go wide is available here. What is complete is the
> HLAE map (§1–§2) — the prerequisite the issue names — and the five specific
> questions it asked (§3). The rest of the engine is **not surveyed**; §5 says
> what that leaves.

---

## 1. HLAE's database, read out of the binary

`AfxHookGoldSrc.dll` registers its pattern keys as C++ static initialisers —
`push <name>; push <result slot>; mov ecx, <map>; call` — which makes the
database recoverable exactly rather than approximately. There are **68 keys**.

### The list in issue #256 was a guess, and several entries were wrong

That list came from `strings`, and the issue said so. Against the real database:

| the issue guessed | actually |
| --- | --- |
| `R_DrawEntitiesOnList_In` / `_Out` | do not exist |
| `R_DrawSkyBox_Begin` / `_End` | do not exist; there is one `R_DrawSkyBoxEx` |
| `S_StartDynamicSound`, `S_Update_` | do not exist; there are `SND_PickChannel` and `GetSoundtime` |
| `CL_ParseServerMessage_CmdRead_MsgReadByte_CallAddrOfs` | does not exist |
| — | six `UnkDrawHud*` keys nobody had listed |

Which is the point of extracting it rather than trusting the names.

### The 46 engine-side keys

**Functions HLAE detours** (14):

```
CL_Disconnect      Host_Init        R_DrawEntitiesOnList   R_PolyBlend
CL_EmitEntities    Mod_LeafPVS      R_DrawParticles        R_RenderView
Draw_DecalMaterial GetSoundtime     R_DrawSkyBoxEx         S_PaintChannels
                                    R_DrawViewModel        S_TransferPaintBuffer
```

plus `host_frametime` and `hw_HUD_GetStudioModelInterface_pStudio`, whose keys
name data but whose detours attach to the function that produces them.

**Resolved but never written to** (the rest): `pEngfuncs`, `ppmove`, `pstudio`,
`engine_ClientFunctionTable`, `CL_ParseServerMessage_CmdRead` (+`_DSZ`),
`p_cmd_functions`, `_Host_Frame`, `R_PushDlights`, `SND_PickChannel`, the six
`UnkDrawHud*`, `clientDll`, `hlExe`, `hwDll`, and the data globals `g_fov`,
`msg_readcount`, `net_message`, `paintbuffer`, `paintedtime`, `r_refdef`, `shm`,
`skytextures`, `soundtime`.

### The 22 game-client keys are `cstrike`, `tfc` and `valve` only

Ten `cstrike_*`, seven `tfc_*`, one `valve_*`, and the three
`hw_HUD_GetStudioModelInterface_*`. **There is no `dod_` key of any kind** —
which is what `docs/goldsrc_death_notices.md` §1 already concluded from the
other direction, now confirmed against the whole database rather than a
`strings` dump.

So on the DoD client side we have the module entirely to ourselves, and
`dodstudio_deathmsg` and `dodstudio_objectives` cannot be colliding with anything
of HLAE's. That is worth knowing with certainty rather than by inference.

---

## 2. The collision model: HLAE detours **prologues**, we detour **mid-function**

`AfxHookGoldSrc.dll` carries `.detourc` and `.detourd` sections and a statically
linked Microsoft Detours. Its install sequence is the standard transaction —
begin, update-thread, then `DetourAttach(&target, hook)` per hook, 34 of them —
and `DetourAttach` rewrites **the first ≥5 bytes of the target function**,
building a trampoline from the instructions it displaced.

That single fact decides the whole risk picture, and it is better than expected:

- **HLAE writes at function entries.** Our detours (`detour::install`) go at a
  *convergence point inside* a function, chosen because that is where a value
  means one thing. Those two are structurally disjoint.
- **The one case that would collide** is us detouring a function entry that HLAE
  also hooks. Nothing we ship does, and `ensure_offset_detour`-style byte
  checking catches it anyway: `E9` is not what our stubs expect to find, so a
  collision fails loudly rather than corrupting both hooks. §5's guardrail —
  every `hw.dll` detour must do the same check — stands, and this is why.
- **Install after HLAE, not before**, still holds. Not because ordering decides
  a winner in the disjoint case, but because being second is what lets the byte
  check see HLAE's jump at all.

### Where HLAE's patterns land in this `hw.dll`

Resolved by applying HLAE's own patterns to the game binary — so these are
addresses in `hw.dll`, which are facts about the game, not about HLAE:

| key | `hw.dll` | detoured |
| --- | --- | --- |
| `CL_Disconnect` | `+0x17850` | yes |
| `R_PushDlights` | `+0x433a0` | no |
| `R_DrawViewModel` | `+0x464c9` | yes |
| `R_DrawParticles` | `+0x46415` | yes |
| `Mod_LeafPVS` | `+0x49642` | yes |
| `R_DrawSkyBoxEx` | `+0x507ae` | yes |
| `S_PaintChannels` | `+0x8cd75` | yes |
| `SND_PickChannel` | `+0x8be00` | no |
| `paintedtime` / `soundtime` / `paintbuffer` | `+0x8cb94` / `+0x8cbe3` / `+0x8e96d` | data |
| `r_refdef` / `skytextures` / `g_fov` | `+0x458ea` / `+0x508f0` / `+0xbfc8` | data |
| `pEngfuncs` | `+0xb232` | data |
| `UnkDrawHudInCall` | `+0xb75b4` | no |

`CL_Disconnect` at `+0x17850` is confirmed twice over: HLAE's pattern matches
there uniquely, and the engine's own demo-stop path calls it
(`hw+0x10923: call 0x1d17850`). The remaining keys use patterns that match many
times because HLAE applies them with a restricted search range this survey does
not reproduce; the tool reports the match count rather than guessing.

> **Nothing of HLAE's is stored in this repository.** The tool reads
> `AfxHookGoldSrc.dll` from the user's own install at run time. Its pattern
> strings are never written out, never committed, and never used as signatures
> of ours. What is recorded is the result of applying them — addresses in the
> *game's* binary. That is the same line `docs/goldsrc_death_notices.md` draws
> ("ideas yes, code no"), drawn here deliberately: lifting the pattern database
> is exactly the act that would put `advancedfx`'s licence in play, and its
> licence is a split whose applicability to `AfxHookGoldSrc` is unresolved.

---

## 3. The five questions the issue asked

### 3.1 The 64-byte command limit — settled, and its name is wrong

**`CLAUDE.md` used to call it "GoldSrc's 64-byte `Cbuf_AddTextToBuffer` limit".
There is no such limit.**

`Cbuf_Init` at `hw+0x272b0`:

```asm
push 0x4000             ; 16,384 bytes
push 0x2d08240          ; the cmd_text sizebuf
push "cmd_text"
call SZ_Alloc           ; hw+0x2ac10 -- sets [buf]=name, [buf+8]=malloc(size)
push 0x4000
push 0x2d08260
push "filteredcmd_text"
call SZ_Alloc
```

GoldSrc's command buffer is **16 KB**, and `hw.dll` contains no string
containing "Cbuf" at all — there is no overflow message to hit.

The 64 is real, but it is the **demo file format**: a Type-3 `ConsoleCommand`
frame carries a fixed `char command[64]`. This project's own reader says so —
`dem-patch/src/demo_parser.rs`'s `parse_console_command` is
`map(take(64usize), …)` — and `native/src/patch/engine.rs` already half-knows it
("64-byte panic is for ConsoleCommand frames, not for director payloads").

**Consequence: not reachable, and there is nothing to raise.** The limit is a
property of the bytes we write into a file the engine parses, not of a buffer we
could grow. Staggering long paths across ticks remains the answer. What changes
is the reasoning. `CLAUDE.md` now says so: the limit is a file-format property
and cannot be raised.

*(Not proven: the engine's own read of that 64-byte field was not located. The
conclusion rests on the format side — our reader, and the pipeline's own naming
— plus the positive proof that `Cbuf` is 16 KB.)*

### 3.2 Interpolation and timing — reachable, and it is one dword

`ex_interp` is **engine-managed**: a per-frame clamp at `hw+0x18ee0` forces it
into a range and **writes the clamped value back through `Cvar_Set`**, printing
`ex_interp forced up to %i msec` or `ex_interp forced down to %i msec`. Setting
it by hand and expecting it to stay is therefore futile, which is worth knowing
before anyone tries.

```asm
hw+0x18ef3  mov edi, 0x32          ; 50 ms, the starting floor
hw+0x18ef8  mov ebx, 0x64          ; 100 ms -- THE CEILING
...
hw+0x18f68  mov ebx, 0xc8          ; 200 ms when a flag at +0x2d5df84 is set
hw+0x18f82  fld  [1000.0]
hw+0x18f88  fdiv [cl_updaterate]   ; the floor is 1000/cl_updaterate, min 1
hw+0x18f9f  fld  [ex_interp]
hw+0x18fa5  fmul [1000.0]          ; in ms
            ...clamp into [edi, ebx], print, Cvar_Set it back
```

The ceiling is a plain `mov r32, imm32` at `hw+0x18ef9`. Raising it is one
dword, with `patch::write_code_bytes`, no detour and no signature beyond
confirming the bytes.

**Why a movie-maker would want it.** A longer interpolation window is smoother
entity motion between snapshots, which is where most spectated-demo ugliness
comes from. 100 ms is a *network* compromise; a demo being rendered offline has
no latency budget to protect.

**Risk:** engine-wide, but narrowly so — it affects entity interpolation and
nothing else, and it is not a cheat vector in a demo. `cl_updaterate` still sets
the floor, so a demo recorded at a low update rate cannot be smoothed below what
it captured.

*Not proven: what the flag at `+0x2d5df84` is. It is written to `1` at
`hw+0x10880` in the demo-start path, so "playing a demo" is the obvious reading
and would mean demos already get the 200 ms ceiling — but that is inference, and
it is exactly the sort of thing that should be checked before the work is
costed.*

### 3.3 Demo playback and parsing — located, not surveyed

*(Later passes: §7 names `CL_ParseServerMessage` and the dispatch table, §8 reads
`CL_ParsePacketEntities`, §10 pins HLAE's span patch inside it. The demo reader
itself is still unopened, per §11.)*

`CL_ParseServerMessage` is at the function containing `hw+0x1aab2`
(`CL_ParseServerMessage: svc_updateuserinfo > MAX_CLIENTS`), and the whole
`svc_*` name table is in `.data` from `hw+0x13afd0`, so the dispatch is
findable. `CL_ParsePacketEntities` is the function containing `hw+0x12ffa`
(`CL_ParsePacketEntities: newindex == MAX_PACKET_ENTITIES` — the same limit
issue #207 is about). The demo reader is around `hw+0x105fd`, bounded by
`Error: Corrupt demo file.` (`hw+0x108fa`) and
`Demo message > MAX_POSSIBLE_MSG` (`hw+0x1149c`).

HLAE resolves `CL_ParseServerMessage_CmdRead` **and its size**, which is the
`_DSZ` shape it uses for a span it intends to overwrite — but it does not
`DetourAttach` it, so whatever it does there is not an entry hook. That is worth
establishing before anything of ours goes near this function.

Not surveyed further. It is the largest remaining item and it deserves its own
pass, not a paragraph.

### 3.4 Entity and decal limits — located, not surveyed

*(Resolved by §9 (#273): the ring's size is not reachable; see
`docs/goldsrc_decals.md` for the shipped `dodstudio_clear_decals`.)*

`r_decals` is registered at `hw+0x46c35` with a default of `4096.0`, alongside
`sp_decals` and `mp_decals`. `Draw_DecalMaterial` is one of the fourteen
functions HLAE detours, which is the first thing anyone touching decals
engine-side needs to know.

`docs/goldsrc_dod_quirks.md` already records the behaviour that matters — the
index rotates and evicts nothing, so lowering `r_decals` mid-demo strands every
decal above the new limit — and the pipeline already works around it from
`init_commands`. Whether the ring's size is reachable was **not** established.

### 3.5 `CL_FlushEntityPacket` — not located

*(Resolved by §8 (#273): located, and the predicate is computable offline.)*

No self-naming string, and no pattern of HLAE's points at it. The condition
under which it fires is already recorded from the demo side, and that is what
the reseq work needed; whether the engine side is reachable remains open.

---

## 4. What is left that is worth doing

*(First-pass table. §7–§9 (#273) resolved the `CL_ParseServerMessage` naming and
the decal and flush rows; see the status table at the top.)*

Subtracting HLAE, and taking §3 at face value:

| item | technique | effort | risk |
| --- | --- | --- | --- |
| raise `ex_interp`'s 100 ms ceiling (§3.2) | immediate rewrite, one dword | very low | narrow: entity interpolation only |
| the 64-byte command limit (§3.1) | **not reachable** — wrong lever | — | — |
| `CL_ParseServerMessage` (§3.3) | unknown; needs its own pass | high | high — every mod, every message |
| the decal ring (§3.4) | unknown; `Draw_DecalMaterial` is HLAE's | medium | `client.dll` has no equivalent, so it is this or nothing |

The general lesson from the client survey holds here too, and more strongly:
**prefer `client.dll` where an equivalent exists.** A change in `hw.dll` affects
the menu, every mod and every session; a change in `client.dll` affects DoD. Of
everything in this document, only `ex_interp` has no client-side equivalent and
a clear payoff.

---

## 5. What is not surveyed *by the first pass*

Stated plainly, because a partial survey presented as complete is worse than
none. **Sections 7-11 close several of these** — read §11 for what is actually
left.

- **Most of `hw.dll`.** `.text` is 1.13 MB against `client.dll`'s 0.67 MB, and
  none of what made the client pass cheap is available: no RTTI, no useful
  exports, and only **36 functions** nameable at all — those that print their
  own name in an error message. Everything else is `sub_*`.
- **Rendering, sound, physics and networking** beyond the HLAE map. Not opened.
- **`R_PushDlights`, `SND_PickChannel` and the six `UnkDrawHud*` keys.** HLAE
  resolves them and does not detour them, which means it reads or patches them
  some other way. What way was not established, and "HLAE resolves it" is not
  the same as "HLAE leaves it alone".
- **Whether the 34 `DetourAttach` sites are all of HLAE's writes.** Detours is
  what it uses for function hooks; a `VirtualProtect`-and-write elsewhere in the
  module would not appear in that count. The `_DSZ` keys (§3.3) suggest at least
  one span patch that is not a Detours hook.
- **The engine's own read of the demo `ConsoleCommand` field** (§3.1).
- **The flag at `+0x2d5df84`** that raises `ex_interp`'s ceiling to 200 ms
  (§3.2).

---

## 6. Reproducing

```
pip install pefile capstone
python goldsrc-hooks/tools/survey_hw_dll.py             # everything
python goldsrc-hooks/tools/survey_hw_dll.py keys        # HLAE's 68 keys
python goldsrc-hooks/tools/survey_hw_dll.py collide     # where they land in hw.dll
python goldsrc-hooks/tools/survey_hw_dll.py findings    # ours, re-checked
python goldsrc-hooks/tools/survey_hw_dll.py parse       # the svc table, and names
python goldsrc-hooks/tools/survey_hw_dll.py entities    # the flush and its predicate
python goldsrc-hooks/tools/survey_hw_dll.py decals      # the decal ring
python goldsrc-hooks/tools/survey_hw_dll.py pin         # HLAE's ambiguous patterns
python goldsrc-hooks/tools/survey_hw_dll.py --hw ... --afx ...
```

`parse`, `entities` and `decals` run without HLAE present, as `findings` does.
`findings` runs without HLAE present and re-checks each address in §3 against
the bytes this build ships, so a different `hw.dll` fails loudly rather than
being described by a document written against another one.

---

## 7. The naming problem, solved: the engine's own dispatch table

§5 says the first pass could name **36 functions**, and #273 said a complete
pass "needs a naming strategy first, not just more disassembly". There is one,
and the engine supplies it.

`CL_ParseServerMessage` does not dispatch through a switch. It dispatches
through a table of 12-byte records:

```c
struct { int opcode; char *name; void (*func)(void); } svc_funcs[60];
```

at `hw+0x13b3b0`, and the dispatch reads its two fields by displacement:

```asm
hw+0x1d498  mov edx, [edi*4 + svc_funcs+4]   ; edi = opcode*3  -> .name
hw+0x1d502  mov edi, [ecx + svc_funcs+8]     ; ecx = opcode*12 -> .func
```

Both displacements appearing in code is what makes this the table the engine
uses, rather than a data shape that merely looks like one — `survey_hw_dll.py
parse` checks for both and refuses the finding without them.

**It names 58 functions in one read.** Not one of them is the target of a
direct `call` anywhere in the image, so a call-graph walk cannot find them at
all; they exist only as entries here.

| opcode | name | handler | | opcode | name | handler |
|---|---|---|---|---|---|---|
| 2 | `svc_disconnect` | `hw+0x1da30` | | 32 | `svc_cdtrack` | `hw+0x1e3b0` |
| 3 | `svc_event` | `hw+0x1e980` | | 33 | `svc_restore` | `hw+0x1e410` |
| 4 | `svc_version` | `hw+0x1dbe0` | | 34 | `svc_cutscene` | `hw+0x1e520` |
| 5 | `svc_setview` | `hw+0x1dc50` | | 35 | `svc_weaponanim` | `hw+0x1e420` |
| 6 | `svc_sound` | `hw+0x1a8f0` | | 36 | `svc_decalname` | `hw+0x1e440` |
| 7 | `svc_time` | `hw+0x1d930` | | 37 | `svc_roomtype` | `hw+0x1e460` |
| 8 | `svc_print` | `hw+0x1dc60` | | 38 | `svc_addangle` | `hw+0x1e210` |
| 9 | `svc_stufftext` | `hw+0x1e1a0` | | 39 | `svc_newusermsg` | `hw+0x1e490` |
| 10 | `svc_setangle` | `hw+0x1e1f0` | | 40 | `svc_packetentities` | `hw+0x1e550` |
| 11 | `svc_serverinfo` | `hw+0x1e1d0` | | 41 | `svc_deltapacketentities` | `hw+0x1e570` |
| 12 | `svc_lightstyle` | `hw+0x1e280` | | 42 | `svc_choke` | `hw+0x1e590` |
| 13 | `svc_updateuserinfo` | `hw+0x1e4a0` | | 43 | `svc_resourcelist` | `hw+0x1e5c0` |
| 14 | `svc_deltadescription` | `hw+0x1cd50` | | 44 | `svc_newmovevars` | `hw+0x1e5e0` |
| 15 | `svc_clientdata` | `hw+0x1da20` | | 45 | `svc_resourcerequest` | `hw+0x1e5f0` |
| 16 | `svc_stopsound` | `hw+0x1e2e0` | | 46 | `svc_customization` | `hw+0x1e600` |
| 17 | `svc_pings` | `hw+0x1ec80` | | 47 | `svc_crosshairangle` | `hw+0x1e230` |
| 18 | `svc_particle` | `hw+0x1e300` | | 48 | `svc_soundfade` | `hw+0x1e610` |
| 19 | `svc_damage` | `hw+0x1e4b0` | | 49 | `svc_filetxferfailed` | `hw+0x1ef90` |
| 20 | `svc_spawnstatic` | `hw+0x1e320` | | 50 | `svc_hltv` | `hw+0x1ece0` |
| 21 | `svc_event_reliable` | `hw+0x1ebb0` | | 51 | `svc_director` | `hw+0x1ecf0` |
| 22 | `svc_spawnbaseline` | `hw+0x1e310` | | 52 | `svc_voiceinit` | `hw+0x1ed60` |
| 23 | `svc_temp_entity` | `hw+0x1e330` | | 53 | `svc_voicedata` | `hw+0x1ed80` |
| 24 | `svc_setpause` | `hw+0x1e340` | | 54 | `svc_sendextrainfo` | `hw+0x1dc00` |
| 25 | `svc_signonnum` | `hw+0x1e370` | | 55 | `svc_timescale` | `hw+0x1ed20` |
| 26 | `svc_centerprint` | `hw+0x1dc80` | | 56 | `svc_resourcelocation` | `hw+0x1e5d0` |
| 27 | `svc_killedmonster` | `hw+0x1e4c0` | | 57 | `svc_sendcvarvalue` | `hw+0x1efc0` |
| 28 | `svc_foundsecret` | `hw+0x1e4d0` | | 58 | `svc_sendcvarvalue2` | `hw+0x1f090` |
| 29 | `svc_spawnstaticsound` | `hw+0x1e3a0` | | 59 | `svc_exec` | `hw+0x1f180` |
| 30 | `svc_intermission` | `hw+0x1e4e0` | | | | |
| 31 | `svc_finale` | `hw+0x1e4f0` | | | | |

`svc_bad` (0) and `svc_nop` (1) carry a NULL handler and are dealt with inline.

**`CL_ParseServerMessage = hw+0x1d300`**, derived as the function containing the
`.func` read rather than asserted.

There is also a per-opcode counter array at `0x2d09de0`, incremented as
`inc [opcode*4 + 0x2d09de0]` for every message parsed — a ready-made histogram
of what a session actually received.

### The self-naming heuristic was too narrow

The first pass matched `Name:` as a *prefix*. The function #273 said was not
located names itself as **`"WARNING:  CL_FlushEntityPacket"`** — the name at the
end, no colon after it. Matching an engine identifier **anywhere in a message**,
and requiring the string to be a message rather than a bare identifier (which
drops 41 OpenGL extension names), raises the count from 36 names to **48
functions located**:

```
CL_CheckCRCs           CL_ParsePacketEntities   Mod_LoadAliasModel   SV_FlyMove
CL_EntityNum           CL_ParseServerInfo       Mod_LoadSpriteGroup  SV_ModelIndex
CL_FlushEntityPacket   CL_ParseTEnt             Mod_NumForName       SV_ParseMove
CL_ParseConsistencyInfo CL_PrecacheResources    Mod_ParseMarksurfaces SV_Physics
CL_ReallocateDynamicData CL_RegisterResources   Mod_PointInLeaf      SV_RecursiveHullCheck
CL_RemoveFromResourceList CL_StartResourceDownloading NET_JoinGroup  SV_StartSound
COM_LoadFile           COM_LoadFileLimit        NET_QueuePacket      SZ_GetSpace
Cmd_AddCommand         Cmd_CheckParm            NET_SendPacket       S_FindName
Cmd_ForwardToServerInternal Draw_TransPic       PM_HullPointContents S_StartDynamicSound
EV_Precache            Host_Error               R_DrawSprite         S_StartStaticSound
Mod_Extradata          Mod_FindName             R_RenderView         SV_ClearResourceLists
R_TextureAnimation     SV_AddSampleToHashedLookupTable  SV_ReallocateDynamicData
```

Caveat, reported rather than hidden: `CL_ParseResourceList` and
`CL_ParseConsistencyInfo` both resolve to `hw+0x1b9f0`. A name appearing in two
places usually means one of them inlined the other; the tool prints such cases
instead of picking.

Between the table and the messages, **106 of `hw.dll`'s functions now have a
name**, against 36 before.

---

## 8. `CL_ParsePacketEntities` and `CL_FlushEntityPacket`

Both located. #273 listed the first as "located but not surveyed" and the second
as "not located".

```
CL_ParsePacketEntities = hw+0x12d90   (names itself at hw+0x12ffa)
CL_FlushEntityPacket   = hw+0x12240   (names itself at hw+0x1228e)
```

`CL_FlushEntityPacket` is called from **three** sites, all inside
`CL_ParsePacketEntities` — `hw+0x12ecf`, `hw+0x13081`, `hw+0x130c4` — and each
is immediately followed by `hw+0x123d0`, which frees the frame's entity array
(`free(frame[+0x24]); frame[+0x24] = 0; frame[0] = 0`).

### The predicate, from the engine's side

The demo side of this was already established for the reseq work. The engine
agrees, and spells it in one place:

```asm
hw+0x12eb7  mov eax, [cls.netchan.incoming_sequence]   ; 0x2d59b20
hw+0x12ebc  mov ecx, [CL_UPDATE_MASK]                  ; hw+0x13afcc
hw+0x12ec2  sub eax, edi                               ; edi = oldpacket
hw+0x12ec4  and eax, 0xff
hw+0x12ec9  cmp eax, ecx
hw+0x12ecb  jl  <normal path>                          ; else fall into the flush
```

so the flush fires when `((incoming_sequence - oldpacket) & 0xff) >=
CL_UPDATE_MASK`.

What is worth recording is that **both constants live in `.data`, not in the
instruction stream**:

```
CL_UPDATE_BACKUP   hw+0x13afc8 = 64
CL_UPDATE_MASK     hw+0x13afcc = 63
```

That is why the predicate is checkable offline at all, and it is what
`survey_hw_dll.py entities` re-checks.

---

## 9. The decal ring

#273 asks whether the ring's **size** is reachable. It is not, and the question
turns out to be the wrong one.

The pool is a fixed array, not a sized allocation: `r_decals` only bounds how
far the rotating index travels before wrapping, and evicts nothing. Measured:

```
R_DecalRemoveAll<by flag>  hw+0x4a000
R_DecalInit                hw+0x49da0
R_DecalUnlink              hw+0x49e80
gDecalPool                 0x2325cb8 .. 0x2341cb8   (0x1c000 bytes)
gDecalCount                0x2342308
sizeof(decal_t)            0x1c  ->  4096 slots = MAX_RENDER_DECALS
decal_t::psurface          +0x04
decal_t::flags             +0x16    bit 0 = permanent; the allocator skips
                                    `flags & 0x81` when rotating
```

What *is* reachable is **emptying** it, which is the thing the pipeline actually
wanted. `R_DecalInit` alone would be a crash — it wipes the pool without
unlinking, leaving every `msurface_t::pdecals` pointing at zeroed structures the
renderer still walks. The engine's own remove functions unlink first, and
`dodstudio_clear_decals` reproduces that loop. See `docs/goldsrc_decals.md`.

The Anniversary engine inlines `R_DecalUnlink` into its remove loops, so it
can't be recovered from the loop there. `decals.rs` finds that build's
standalone copy by its own signature instead (`goldsrc_decals.md` §5).
`R_DecalInit`'s signature matches both.

---

## 10. HLAE's non-detour write, pinned

§5 recorded a loose end: the `_DSZ` keys imply at least one HLAE write that is
not a Detours hook, and `CL_ParseServerMessage_CmdRead` is resolved but never
`DetourAttach`ed.

Its pattern matches four places, which the first pass could not narrow. With
`CL_ParseServerMessage` named, one of them is inside it and the rest are not:

```
CL_ParseServerMessage_CmdRead: 4 matches -- PINNED
    CL_ParseServerMessage (hw+0x1d3e6)
    hw+0x58f2f
    hw+0xe18c4
    hw+0xe19ff
```

`hw+0x1d3e6` is seven bytes:

```asm
hw+0x1d3e6  8b df              mov ebx, edi
hw+0x1d3e8  e8 e3 d4 00 00     call hw+0x2a8d0      ; MSG_ReadByte
hw+0x1d3ed  8b f0              mov esi, eax         ; esi = the svc opcode
hw+0x1d3f0  83 fe ff           cmp esi, -1
```

`hw+0x2a8d0` is `MSG_ReadByte`, identified from its body rather than its name:
it reads `msg_readcount` (`0x2d08228`) against `net_message.cursize`
(`0x27a2730`), sets `msg_badread` (`0x2d0822c`) and returns -1 on overrun, and
otherwise returns `net_message.data[msg_readcount++]` (`0x27a2728`).

So HLAE overwrites **the instruction pair that fetches each svc opcode**, in the
middle of `CL_ParseServerMessage`, with a span patch rather than a prologue
detour. That is the collision §2 warns about, now with an address: anything of
ours that wants to watch the message stream must not touch `hw+0x1d3e6 .. +7`,
and the `msg_readcount` globals above are the cheaper place to watch from.

---

## 11. What is still not surveyed

Shorter than §5, and deliberately not empty.

- **Most of `hw.dll` still.** 106 named functions out of several thousand.
- **The demo reader** (`hw+0x105fd`, bounded as §3.3 describes) is located and
  still not opened.
- **`R_PushDlights`, `SND_PickChannel` and the six `UnkDrawHud*` keys.**
  Unchanged from §5: HLAE resolves and does not detour them, and how it uses
  them is still unestablished.
- **The other three `CL_ParseServerMessage_CmdRead` matches** (`hw+0x58f2f`,
  `hw+0xe18c4`, `hw+0xe19ff`) are in unnamed functions. They are almost
  certainly other `MSG_Read*` call sites rather than anything HLAE wants, but
  that is inference.
- **Whether the 34 `DetourAttach` sites plus this one span patch are all of
  HLAE's writes.** One was found by pulling on the `_DSZ` thread; nothing says
  it was the only one.
- **The engine's own read of the demo `ConsoleCommand` field** (§3.1), and the
  **`ex_interp` flag at `+0x2d5df84`** (§3.2). Both unchanged.

*(#300 picks these up. §12 answers the HLAE items — the eight keys and the
census of writes — for both builds; §13 is the demo reader; §3.2's flag is
answered by `docs/goldsrc_ex_interp.md` §2–§2b.)*

---

## 12. Every HLAE write, per build (#300)

#300 asked two things of HLAE, and the 2026-09-28 review (D6) narrowed it to
exactly those, **per build**: how HLAE uses `R_PushDlights`, `SND_PickChannel`
and the six `UnkDrawHud*` keys, which it resolves but never `DetourAttach`es;
and whether the 34 `DetourAttach` sites plus §10's span patch are all of its
writes.

**Subjects:** `AfxHookGoldSrc.dll` from **HLAE 2.192.4** (449,536 bytes, the
one install on this machine), against both engines: the pre-Anniversary
`hw.dll` (§1–§11's subject) and the 25th Anniversary `hw.dll` from the
*POST-Anniversary for Movies* install (3,598,176 bytes, `ImageBase
0x10000000`). Offline, `pefile` + `capstone`; `survey_hw_dll.py writes` and
`spans` reproduce it.

### 12.1 First: §1's HLAE was an older build

§1–§2 and §10 read a different `AfxHookGoldSrc.dll`, and HLAE has moved. Against
2.192.4:

| §1 says | 2.192.4 has |
| --- | --- |
| 68 keys, 46 engine-side, 22 game-client | **69**: 50 engine-side, 19 game-client |
| 34 `DetourAttach` sites | **40** |
| `R_DrawEntitiesOnList_In`/`_Out`, `R_DrawSkyBox_Begin`/`_End`, `S_StartDynamicSound`, `S_Update_`, `CL_ParseServerMessage_CmdRead_MsgReadByte_CallAddrOfs` do not exist | all seven exist |
| `SND_PickChannel`, `GetSoundtime` are keys | **neither is** |

So §1's "the issue guessed / actually" table was right about the build it read
and is wrong about this one. **The `SND_PickChannel` half of item 2 has no
answer to give: 2.192.4 does not resolve it at all.**

The reason for most of the drift is that this HLAE supports both engines. It
picks a branch per key from one test, made once at `hw.dll` load: **does
`hw.dll` contain the string `A3D.DLL`?** The pre-Anniversary build does, the
Anniversary build does not. (Proven: the resolver stores the result of that
search in the flag every two-branch key reads.) Most keys carry one pattern per
branch, which is why `collide` reports so many `0 matches` lines — each is the
*other* build's pattern, not a miss.

### 12.2 The census (item 3)

The question was whether a `VirtualProtect`-and-write exists outside the
`DetourAttach` count. The way to close it is from the API side: `VirtualProtect`
is imported once, and everything that changes a page's protection has to reach
it.

It is referenced from **8 direct call sites and one jump thunk, in 9
functions**. Four are statically linked Microsoft Detours (transaction begin,
commit, `DetourAttachEx`, and the thunk they share). The other five are HLAE's
own, and they are the complete list of its write primitives:

| primitive | what it writes | call sites |
| --- | --- | --- |
| Detours `DetourAttach` | rewrites a function's first ≥5 bytes, trampoline elsewhere | **40** |
| trampolined jump | copies a span to a new trampoline, NOPs it, writes `E9 rel32` over its start | **2** |
| in-place jump | NOPs a span and writes `E9 rel32`; no trampoline — HLAE re-creates the displaced instructions in a stub of its own | **11** (6 into `hw.dll`, the rest into its own stubs) |
| bracketed write | unprotect *n* bytes, plain store, restore | **12** |
| import-table slot | one pointer, through an import-hook manager | 2, which the manager runs over **4** modules |
| executable allocation | HLAE's own heap, never foreign memory | 4 |

Every call site of every one of those is attributed in §12.3. **That closes item
3 for protection-changing writes, proven:** there is no `VirtualProtect` call in
the module that is not one of these, and none of these has an unattributed
caller.

What a `VirtualProtect` census *cannot* see is a store into memory that is
already writable. Two kinds turned up by following the key slots, and they are
in the table too: **console command handlers** (a `cmd_function_t` node's
handler field, heap) and **slots of the engine's `cldll_func_t` copy** (`.data`;
HLAE brackets these anyway). A scan for every `mov [reg+disp], <AfxHookGoldSrc
code address>` finds only six, all inside one of HLAE's own objects. *Not
proven:* a store into writable memory through a pointer HLAE computes at run
time from something that is not a key would not show in either search. Nothing
suggests one exists.

### 12.3 The map

`hw.dll` addresses, per build. "At load" means HLAE's `hw.dll` installer, which
runs once when `hw.dll` is loaded, before `client.dll` exists; "lazy" means on
the first use of a console command, and never otherwise.

**Prologue detours in `hw.dll`** — the §2 model, unchanged:

| target | builds | when |
| --- | --- | --- |
| `CL_Disconnect`, `Host_Init`, `_Host_Frame`, `Mod_LeafPVS`, `R_DrawParticles`, `R_DrawViewModel`, `R_PolyBlend`, `R_RenderView` | both | at load |
| the function `cl_enginefuncs` slot 69 points at (`pfnHookEvent`) | both | at load |
| `R_DrawEntitiesOnList`, `R_DrawSkyBoxEx` | **pre-Anniversary only** | at load |
| `R_StudioSetHeader`, `R_SetRenderModel`, `R_SetupRenderer` (`engine_studio_api_t`) | both | when the engine asks `client.dll` for its studio interface |
| `S_PaintChannels`, `S_TransferPaintBuffer`, `S_StartDynamicSound` | both | lazy: when HLAE starts recording sound |
| `Draw_DecalMaterial` | both | lazy: `mirv_decalfilter` or `mirv_noadverts` |
| `CL_EmitEntities` | both | lazy: `dem_forcehltv` |
| the function `cl_enginefuncs` slot 66 points at (`pfnWeaponAnim`) | both | its installer is named for `cstrike`; the gate was not traced |

That is 20 of the 40 call sites. The other 20 are 19 in `client.dll`
(`cstrike_*`, `tfc_*`, `valve_*`, all game-gated) and one on the game window's
procedure in `SDL2.dll`. The three studio-interface targets are named by HLAE's
own failure messages, not by anything in `hw.dll`.

**Span patches in `hw.dll` `.text`** — the shape §10 found, and it was not
alone. Each overwrites the listed span in the middle of a function:

| key | pre-Anniversary | 25th Anniversary | span | when |
| --- | --- | --- | --- | --- |
| `UnkDrawHudIn` | `hw+0xb75b4` | `hw+0x25d2d2` | 5 (a `call`) | at load |
| `UnkDrawHudOut` | `hw+0xb7639` | `hw+0x25d34f` | 5 (a `call`) | at load |
| `R_DrawEntitiesOnList_In` | — | `hw+0x244354` | 9 | at load |
| `R_DrawEntitiesOnList_Out` | — | `hw+0x244492` | 12 | at load |
| `R_DrawSkyBox_Begin` | — | `hw+0x251521` | 8 | at load |
| `R_DrawSkyBox_End` | — | `hw+0x2516e6` | 6 | at load |
| `CL_ParseServerMessage_CmdRead` | `hw+0x1d3e6` | `hw+0x1a7ddc` | 7 / 11 | lazy: `mirv_voice_block` |

The containing functions: pre-Anniversary `hw+0xb74e0` (both `UnkDrawHud*`) and
`hw+0x1d300` (`CL_ParseServerMessage`); Anniversary `hw+0x25d1f0`,
`hw+0x244130` (`R_DrawEntitiesOnList`), `hw+0x2513c0` (`R_DrawSkyBox`) and
`hw+0x1a7cb0`.

The pattern is a substitution: **where the Anniversary engine's function cannot
take a prologue detour cleanly, HLAE swaps it for two span patches** —
`R_DrawEntitiesOnList` and `R_DrawSkyBoxEx` are detoured on the old engine and
span-patched on the new one. So the per-build collision maps differ in kind,
not just in address.

§10's span patch, for the record, **is lazy**: it is installed the first time
`mirv_voice_block` runs, which strips voice data from blocked players by
advancing `msg_readcount` past it. In a session where nobody types that command,
`hw+0x1d3e6` is untouched. On the Anniversary build the span is 11 bytes, not 7
— a `mov [ebp-0x10c], ebx` ahead of the same `call MSG_ReadByte`.

**Data writes** (writable already):

| what | where | when |
| --- | --- | --- |
| `cldll_func_t` slot 19 (`V_CalcRefdef`) | the engine's copy of the client table | lazy: `__mirv_force_players_solid` |
| slot 15 (`CL_IsThirdPerson`) | same | lazy: `dem_forcehltv` |
| slot 6 (`HUD_PlayerMove`) | same | lazy: `__mirv_moveto` |
| `skytextures[6]` (24 bytes) | `hw.dll` `.data` | swapped around each sky draw, from the sky hooks |
| `msg_readcount` | `hw.dll` `.data` | per message, after `mirv_voice_block` |
| handler of `connect`, `dem_forcehltv`, `startmovie`, `endmovie` | `cmd_function_t` nodes | once, right after `Host_Init` returns (from HLAE's `Host_Init` hook) |

**Import-table slots in `hw.dll`:** `KERNEL32!LoadLibraryA`,
`KERNEL32!GetProcAddress` and `SDL2!SDL_GL_GetProcAddress`, at load. The same
manager hooks imports of `hl.exe`, `SDL2.dll` (`GetProcAddress`,
`CreateWindowExW`, `DestroyWindow`, `SetCursorPos`, `SwapBuffers`) and
`client.dll`.

**One write in `client.dll`, and it is DoD's too.** `__mirv_demozoom` puts a
trampolined jump over `client.dll`'s exported `Demo_ReadBuffer` (6 bytes, lazy).
Every GoldSrc client exports that, so §1's "on the DoD client side we have the
module entirely to ourselves" is true of HLAE's *patterns* — there is still no
`dod_` key — but not of every HLAE write. It needs a command nobody types.

### 12.4 Item 2, answered

- **`R_PushDlights` — read, never written.** HLAE stores its address beside
  `R_RenderView`'s and calls it from its own `R_RenderView` hook (an indirect
  `call` through the stored pointer), presumably to rebuild dynamic lights for a
  second render pass. Pre-Anniversary `hw+0x433a0`; Anniversary `hw+0x241cd0`,
  one match each.
- **`SND_PickChannel` — gone** from 2.192.4 (§12.1).
- **The six `UnkDrawHud*` keys — two span patches**, and the names explain
  themselves once the offsets are read. `In` and `Out` are two 5-byte `call`
  instructions in the same function, `0x85` bytes apart on the old engine and
  `0x7d` on the new; `InCall`/`OutCall` are those calls' targets and
  `InContinue`/`OutContinue` are the addresses just after them. Each `call` is
  replaced by a jump into a stub that runs HLAE's code, makes the original call
  itself, and jumps back to `Continue` — a bracket around one piece of HUD
  drawing. Installed at load, on both builds, in every HLAE session. Same shape
  as §10, so item 2's hunch was right for six of the eight.

### 12.5 What it means for `goldsrc-hooks`

**Three shared slots, all safe by chaining.** `goldsrc-hooks` and HLAE write the
same location in three places:

| location | ours | HLAE's |
| --- | --- | --- |
| `hw.dll` IAT `LoadLibraryA`, `GetProcAddress` | `engine.rs` | at load |
| the `connect` command's handler | `connect_guard.rs` | after `Host_Init` |
| `cldll_func_t` slot 19 | `engine.rs` (`SLOT_CALC_REFDEF`) | only after `__mirv_force_players_solid` |

In every one, **both sides save the slot's current value and call through it**
— `hook_import` stores `*slot` as the real function, `connect_guard`'s `wrap`
keeps the handler it replaces, and HLAE's three writers do the same — so either
install order gives a working chain. *Inferred, not traced:* that neither side
restores its saved value while the other's hook is above it. Restoring would
silently unhook the other.

`pfnHookEvent` is a near miss rather than a collision: HLAE detours the
*function* slot 69 points at, at load; `missing_shots.rs` swaps the *slot*,
later. Ours calls through what it found, which is the engine's function, whose
prologue now jumps to HLAE. Also chain-safe.

**No byte overlap with any span.** None of the six `hw.dll` span sites, nor
their containing functions, has an address of ours within 32 bytes: checked
against everything the `verify_*` tools print — twelve for the pre-Anniversary
build, the four that take `--anniversary` for the other. That second list is not
complete, so on the Anniversary build this is "nothing found", not "proven
disjoint". `detour.rs`'s byte check is still the backstop: an `E9` where a stub
expected the original bytes fails loudly.

The practical rule §10 gave for the message stream — don't touch the
`CmdRead` span, watch `msg_readcount` instead — holds on both builds, with the
Anniversary span 11 bytes long. And there is now a second region with the same
warning on the Anniversary build only: **`R_DrawEntitiesOnList` and
`R_DrawSkyBox` are not detour targets there; their insides are HLAE's.**
