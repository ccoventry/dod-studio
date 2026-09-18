# Silencing and hiding things DoD draws, without editing game files

`dodtools_mute_voice_commands` and `dodtools_hide_crosshair`, and why each needed a
patch rather than a setting. Companion to `docs/goldsrc_scoreboard.md`, which
covers `dodtools_hide_scoreboard`.

Everything here is from offline analysis of DoD 1.3's `client.dll` (`pefile` +
`capstone`, the house method in `docs/goldsrc_client_dll_internals.md` §10),
checked by `goldsrc-hooks/tools/verify_voice_crosshair_offsets.py`.

---

## 1. The pattern all three share

Each of these was already solvable by editing a file the game ships:

| what | the file workaround |
|---|---|
| scoreboard | `dod/resource/ui/ScoreBoard.res` — set `wide 0`, `tall 0`, `visible 0` |
| spectator bars | `dod/resource/ui/Spectator.res` + `BottomSpectator.res` — same trick |
| voice commands | overwrite every `player/us*.wav`, `player/brit*.wav`, `player/ger*.wav` with a blank sound |
| crosshair | — (no file to edit; the cvar exists but does not stick) |

Editing those files works and is what was being done. What it costs is that a
**per-take** decision becomes a **persistent** change to the install, has to be
undone to get the normal behaviour back, and — for the voice commands —
destroys shipped game content. `CLAUDE.md`'s standing rule is that the game's
own files belong to the user; this is the same principle applied to the rest of
the install.

---

## 2. `dodtools_mute_voice_commands`

### Where the sound comes from

DoD plays voice commands through exactly two client event callbacks:

```text
pfnHookEvent("events/misc/usvoice.sc",  client+0xb3f0)   ; US *and* British
pfnHookEvent("events/misc/gervoice.sc", client+0xb5d0)   ; German
```

Behind them are three contiguous 28-entry `const char*` tables:

| table | first entries |
|---|---|
| `+0x1c55c8` | `player/usattack.wav`, `player/ushold.wav`, `player/usfallback.wav`, … |
| `+0x1c5638` | `player/britattack.wav`, `player/brithold.wav`, `player/britfallback.wav`, … |
| `+0x1c56a8` | `player/gerattack.wav`, `player/gerhold.wav`, `player/gerfallback.wav`, … |

Those are precisely the files the blank-`.wav` workaround overwrites, which is
the evidence that this is the whole mechanism and not one path of several.
Index 0 of each table is the empty string — the game's own "no sound" entry.

The US and British voices share a callback; which of the two tables it indexes
comes from an event parameter, so one patch site covers both.

### The patch

Each callback reaches `pEventAPI->EV_PlaySound` through
`call dword ptr [ebx]` — `FF 13`. Those two calls become `90 90`.

```text
client+0xb489   FF 13   ->   90 90     (US / British)
client+0xb664   FF 13   ->   90 90     (German)
```

The stack needs no other adjustment, because argument cleanup is caller-side
and separate — `add esp, 0x20` at `+0xb48b` and `add esp, 0x24` at `+0xb66d`.
Removing the call does not move either.

### Why the call and not the callback

A `ret` at the top of each callback would be simpler and is wrong. Everything
*after* the call still matters, and what it does is print the chat line.
Patching only the call reproduces the blank-`.wav` behaviour exactly — audio
gone, everything else as it was. Stubbing the callback would have taken the
chat line with it, silently.

### What the tail actually does, and when

Read rather than assumed, because it decides what this setting looks like in
practice. After the sound, the US/British callback:

1. gets the speaker's entity and the local player, and their player info;
2. **returns if the speaker is not on your team**;
3. **returns if the observer-mode global is non-zero — i.e. whenever you are
   spectating**;
4. returns if the speaker is further away than a fixed distance;
5. otherwise formats `"%c%s%s%s"` from `"(%s1) "`, the player's name and
   `": %s2"`, and prints it with the `#VOICE` prefix and the matching
   `#Voice_subtitle_*` string.

So the "subtitle" is **the chat line** — `(PlayerName): Fire in the hole!` —
and step 3 means it **never appears while spectating at all**. In an HLTV demo
this setting removes the sound and there was never any text; in a POV demo it
removes the sound and the chat line stays, exactly as it did with blanked
`.wav` files.

If the chat line should go too, that is a `ret` at the top of each callback
rather than a NOP over the call — a second mode, not a different design.

### Scope

Voice **commands** only. Pain, death and hurt sounds are different events and
are untouched, as is player voice chat (`voice_modenable`) and its
`sprites/voiceicon.spr` speaker icon, which is `CVoiceStatus` — a separate
system that has nothing to do with these callbacks.

---

## 3. `dodtools_hide_crosshair`

### Why the stock cvar is not enough

DoD registers a `crosshair` cvar. Setting it to 0 appears to work for one
frame. `CHud::Redraw` **silently forces the value back** every frame — it is
one of three cvars in that enforcement block
(`docs/goldsrc_client_dll_survey.md` §8, step 3). The other two in the same
block, `r_drawentities` and `cl_lw`, do not merely reset: they quit the game,
which is why `native/src/patch/cfg_scan.rs` refuses them as typed commands.

So the gap is not "there is no setting". It is "the setting is overwritten
before it can take effect".

### The patch

`CHudDoDCrossHair` is an ordinary HUD element, reached through `CHud::Redraw`'s
element walk with `Draw` in vftable slot 3. Its `Draw` begins:

```asm
client+0x2cd20  mov  eax, [esp+4]      ; 8B 44 24 04
client+0x2cd24  push esi               ; 56
client+0x2cd25  push eax
client+0x2cd26  mov  esi, ecx
client+0x2cd28  call client+0x2d2b0
```

Writing `33 C0 C2 04 00` over the first five bytes makes it
`xor eax, eax; ret 4` — **byte-for-byte `CHudBase::Draw`**, the do-nothing base
implementation at `client+0x21940` that every element inherits and most
override. The element stays in the list, still gets called, and does what an
element that draws nothing does. Reverting restores the five bytes the
signature already proved were there.

### Why the function and not the vftable

Issue #265's idea — write `CHudBase::Draw`'s address into vftable slot 3 — is
one dword and cheaper. It also needs the vftable's address at runtime, which
means resolving RTTI in the loaded module or signature-matching the constructor
that stores it. Patching the function needs neither. The general
`dodtools_hudelement` command in #265 is still worth having; this is not it and
does not block it.

### Both the POV and the spectator crosshair

One function draws both, and the stub is at its first instruction, so both die.
`Draw` branches on the observer-mode global at `client+0x1e88d4`:

| observer mode | what happens |
|---|---|
| `0` (not spectating) | the POV crosshair — **the only path that reads the `crosshair` cvar** |
| `3` or `4` | `client+0x2d1f0`, the spectator crosshair (roaming and in-eye in HL's numbering) |
| anything else | nothing is drawn |

**This answers the open question in #219.** POV and HLTV first-person differ
because the spectator branch never consults the `crosshair` cvar. Even if
`CHud::Redraw` were not forcing the value back every frame, `crosshair 0` could
not have hidden the spectator crosshair — no code path reads it there.

### What is not covered

DoD also calls the engine's own `pfnSetCrosshair` (`gEngfuncs[13]`) from its
weapon-sprite code, once with a null sprite (clearing it) and once with a real
one. That is a second, engine-drawn crosshair, this does not touch it, and
whether it is ever visible has **not** been established. A live test settles it
in seconds: if anything is still on screen with this set to 0, that is what it
is.

---

## 4. Re-applied every frame, from the bytes

All three settings (including `dodtools_hide_scoreboard`) are handed to their
`apply` every frame rather than compared against a cached flag.

That is not defensive habit. **The engine unloads and reloads `client.dll`
between demos**, and a reloaded module comes back with the original bytes. A
cached "already patched" belief would leave every one of these working for the
first demo of a session and quietly stopping from the second onward — a failure
you would only notice in the finished footage. After the first scan the check
is a short byte compare, so the cost is nothing.

`commands.rs`'s `poll_code_patch` is that shared loop; `apply` returns whether
it wrote, so the log line stays change-triggered.

---

## 5. Not done: the spectator bars

`mirv_disable_specmenu` is HLAE's equivalent, and it fails on DoD with
`"Error: Hook not installed."` — its per-game
`TeamFortressViewport_UpdateSpecatorPanel` address exists for `tfc` and `valve`
and not for `dod`. (`AfxHookGoldSrc.dll` contains no `"dod"` string at all;
its pattern database is cstrike 14 entries, tfc 7, valve 1, dod 0.)

DoD has the machinery: `CDoDSpectatorGUI` (vftables `+0x1aab84`/`+0x1aab3c`),
`CSpectatorGUI`, `ISpectatorInterface`, `DoDViewport`, `TeamFortressViewport`,
`CBackGroundPanel@TeamFortressViewport`, and the two `.res` files above.

What has **not** been established is the per-frame function that decides the
panel is visible — DoD's equivalent of `UpdateSpectatorPanel`. The construction
path is known (`+0x82df9` loads `Spectator.res`; the constructor is at
`+0x1d9ec`), but that runs once, and something re-shows the panel afterwards.

The likely shape is stubbing `PaintTraverse` on the spectator GUI's vgui2
panel vftable, which paints a panel *and its children* and so would take the
bars, the dropdowns and the buttons together in one dword. That needs the
vgui2 `Panel` vtable layout for this build, which is the remaining work.

Lower priority than it looks: unlike the voice commands, the spectator bars
already have a working `.res` workaround.
