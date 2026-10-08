# Silencing and hiding things DoD draws, without editing game files

The shared pattern behind the `client.dll` patches in `goldsrc-hooks` that hide
or silence something DoD draws, and why each needed a patch rather than a
setting. This doc keeps the pattern, `dodstudio_mute_voice_commands` and the
per-frame re-apply loop; the other topics have their own docs:

| topic | command(s) | doc |
|---|---|---|
| scoreboard | `dodstudio_hide_scoreboard` | `docs/goldsrc_scoreboard.md` |
| crosshair, and the spectator crosshair under `spec_match_pov` | `dodstudio_hide_crosshair`, `dodstudio_spec_match_pov` | `docs/goldsrc_crosshair.md` |
| any HUD element by name, and the notes on the `CHudDoD*` elements that are not offered | `dodstudio_hide_hudelement` | `docs/goldsrc_hud_elements.md` |
| the spectator bars (VGUI2 panel) | `dodstudio_hide_spectator_bars` | `docs/goldsrc_spectator_bars.md` |
| objective icons and timer position | `dodstudio_objectives` | `docs/goldsrc_objective_icons.md` |

Everything here is from offline analysis of DoD 1.3's `client.dll` (`pefile` +
`capstone`, the house method in `docs/goldsrc_client_dll_internals.md` §10),
checked by `goldsrc-hooks/tools/verify_voice_crosshair_offsets.py` and
`verify_spectator_crosshair_offsets.py`.

---

## 1. The pattern they all share

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

## 2. `dodstudio_mute_voice_commands`

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

## 3. Re-applied every frame, from the bytes

Every one of these settings (the scoreboard, voice, crosshair, spectator crosshair and HUD element hides, and the spectator bars filter) is handed to its
`apply` every frame rather than compared against a cached flag.

That is not defensive habit. **If the engine ever unloads and reloads
`client.dll`**, a reloaded module comes back with the original bytes, and a
cached "already patched" belief would leave every one of these working right
up until that point and quietly stopping after — a failure you would only
notice in the finished footage. Measured 2026-09-18 (`docs/goldsrc_dod_quirks.md`):
a plain demo-to-demo transition does **not** reload `client.dll` — five game
sessions, five load lines, none mid-session — so this guards against whatever
*does* (a mod change, returning to the menu), not against loading a new demo.
After the first scan the check is a short byte compare, so the cost is
nothing either way.

`commands.rs`'s `poll_code_patch` is that shared loop; `apply` returns whether
it wrote, so the log line stays change-triggered.

For the spectator crosshair (`docs/goldsrc_crosshair.md` §2) the same loop earns its keep twice over: polling is also how it notices
`cl_xhair_style` being changed under it, with no cvar callback needed.


---

## 4. The spectator bars

Done: `dodstudio_hide_spectator_bars` (2026-09-30), by vgui2's own
`IPanel::PaintTraverse`. See `docs/goldsrc_spectator_bars.md`. This section used
to hold the pre-shipping guesswork; the durable parts of it (HLAE's
`mirv_disable_specmenu` failing on DoD with `"Error: Hook not installed."`, and
the `AfxHookGoldSrc.dll` pattern counts) moved to that doc's history.
