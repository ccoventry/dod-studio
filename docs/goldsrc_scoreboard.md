# Hiding DoD's scoreboard without editing `ScoreBoard.res`

How `dodtools_hide_scoreboard` works, what it does not cover, and the evidence for
each claim. Everything here is from offline analysis of DoD 1.3's `client.dll`
(`pefile` + `capstone`, the house method in
`docs/goldsrc_client_dll_internals.md` §10) plus a parse of real demos.

---

## 1. The problem

A POV demo replays whatever the recording player typed. A key bound to
`+showscores` is no exception: pressing TAB during a match writes a **Type-3
`ConsoleCommand` frame** into the demo, and playback executes it. Every time
the player checked the score, the scoreboard covers the shot.

HLTV demos do not have this problem, because nobody was pressing anything.
That asymmetry is what makes it look like a rendering quirk rather than
recorded input.

This is measurable, not inferred — parsing the frame stream of real demos and
counting `ConsoleCommand` frames whose text contains `showscores`:

| demo | `showscores` command frames | in `svc_stufftext` |
|---|---|---|
| `test_director_cmds_pov_source.dem` (POV) | 126 | 0 |
| `bandits-ktps5w10-map2-anzio-over-allies-milo.dem` (POV) | 294 | 0 |
| a pipeline-recorded demo | 0 | 0 |

All of them are plain command frames; none travel as stuffed text.

The workaround before this was to edit `dod/resource/ui/ScoreBoard.res` and
push the dialog off-screen. It works, but it is a persistent edit to a game
file for a per-take decision, and it has to be undone to read a score again.

---

## 2. Why there is no cvar for it

There isn't one, and the reason is structural.

**DoD's scoreboard is not a HUD element.** It is VGUI2:

```
IScoreBoardInterface
  └── CClientScoreBoardDialog
        └── CDoDClientScoreBoardDialog
```

recovered from the RTTI type descriptors at `+0x1c1448`, `+0x1c146c` and
`+0x1c1494`, and laid out from the resource named at `+0x1d2ef8`,
`Resource/UI/ScoreBoard.res`. It appears **nowhere** in the HUD element
inventory of `docs/goldsrc_client_dll_survey.md` §1, so `CHud::Redraw`'s
element walk never reaches it and `hud_draw 0` does nothing to it.

The only score-related cvar DoD's client registers at all is
`spec_scoreboard` (`cvar_t*` at `+0x1174fc`), which belongs to
`CHudSpectator`, is read only in spectator mode, and *toggles on change*
rather than gating anything.

So there was nothing to set.

---

## 3. What is patched

`+showscores` and `-showscores` are registered next to each other, in the
client's command-registration block:

```asm
+0x3d842  push 0x193c650          ; the handler
+0x3d847  push 0x19cf9d8          ; "+showscores"
+0x3d84c  call [pfnAddCommand]
+0x3d852  push 0x193c670          ; the handler
+0x3d857  push 0x19cf9cc          ; "-showscores"
+0x3d85c  call [pfnAddCommand]
```

The show handler's entire body is eight instructions:

```asm
client+0x3c650  push 0x1a8aa24        ; the in_score kbutton
client+0x3c655  call client+0x3bae0   ; KeyDown bookkeeping
client+0x3c65a  mov  ecx, [gViewPort] ; client+0x19d564
client+0x3c660  add  esp, 4
client+0x3c663  test ecx, ecx
client+0x3c665  je   client+0x3c66c   ; <- 74 05
client+0x3c667  mov  eax, [ecx]
client+0x3c669  jmp  [eax+0x18]       ; ShowScoreBoard
client+0x3c66c  ret
```

Turning the `je` (`74`) into a `jmp` (`EB`) makes that branch unconditional,
so the function always falls through to its own `ret`. **One byte**, and the
displacement needs no adjustment because the landing site does not move.

### Why the `je` and not the entry point

Writing `C3` over `+0x3c650` would be just as short and is worse. The `call`
at `+0x3c655` is the engine's `+`/`-` key bookkeeping for the `in_score`
kbutton; skipping it would leave that state claiming the key is up while it is
held. Patching the `je` removes the virtual call and nothing else.

### Why `-showscores` is left alone

Hiding an already-hidden dialog is a no-op, so blocking it buys nothing. More
usefully, leaving it working makes the suppression **self-correcting**: a
scoreboard already on screen when the setting is switched on disappears at the
player's next key release, rather than sticking until the next demo.

### The signature

```
68 ?? ?? ?? ?? E8 ?? ?? ?? ?? 8B 0D ?? ?? ?? ?? 83 C4 04 85 C9 74 05 8B 01 FF 60 18
```

The trailing `FF 60 18` is load-bearing. `-showscores` is **byte-identical**
up to that point and differs only in the vftable displacement (`FF 60 2C`), so
a signature that stopped short would match both. Each matches exactly once in
the analysed image, and `scan::find_unique` refuses a pattern that does not.

`goldsrc-hooks/tools/verify_scoreboard_offsets.py` checks all of this against a
real `client.dll`, reading every constant out of `scoreboard.rs` rather than
restating it:

```
  OK   the signature matches exactly once, at +0x3c650
  OK   +0x3c665 is `je 0x193c66c`, a two-byte je rel8
  OK   the je jumps to +0x3c66c, which is the function's own `ret`
  OK   +0x3c650 is the handler registered for "+showscores"
  OK   "-showscores" is a separate function (+0x3c670) the signature does not match
  OK   with 0xeb written, every later instruction still starts where it did
SIGNATURE VERIFIED
```

The fourth check is the one that matters most: it resolves the handler
independently, by finding the pointer pushed next to the `"+showscores"`
string, rather than trusting that the signature found the right function.

---

## 4. Using it

```
dodtools_hide_scoreboard 1     block +showscores
dodtools_hide_scoreboard 0     back to the game's own behaviour (the default)
dodtools_hide_scoreboard       report the current state
dodtools_debug_status                report it alongside every other setting
```

It is a cvar, so it also takes `+dodtools_hide_scoreboard 1` on the launch line or
a line in any `.cfg` the session execs — which is the useful form for an
unattended capture, since it is then set before the first demo loads.

The setting is re-applied every frame from the byte itself rather than from a
cached flag. That is not belt-and-braces: if `client.dll` is ever unloaded and
reloaded, a reloaded module comes back with the stock byte, and a cached
"already suppressed" belief would leave the scoreboard working again from
that point onward, silently. Measured 2026-09-18
(`docs/goldsrc_dod_quirks.md`): a plain demo-to-demo transition does **not**
reload `client.dll` — five game sessions, five load lines, none mid-session —
so this guards against a mod change or returning to the menu, not against
loading a new demo.

---

## 5. What this does not cover

Stated plainly, in the habit `docs/goldsrc_death_notices.md` set.

- **Round-end and intermission.** DoD shows the scoreboard by itself at the end
  of a round. That reaches the dialog by a different call site, which has not
  been traced.
- **`spec_scoreboard`.** `CHud::Redraw` toggles the VGUI scoreboard when that
  cvar's value changes (`docs/goldsrc_client_dll_survey.md` §8, step 2). Also
  untouched.
- **A board already on screen.** Blocking the show does not hide what is
  already up; the player's next key release does. See §3.

If a take ever needs "no scoreboard under any circumstance", the blunter
instrument is stubbing `CDoDClientScoreBoardDialog`'s paint slot in its
vftable (primary vftable at `+0x1aa2e4`), which is the same one-dword idea as
issue #265. That is a bigger change and is not what this is.

- **Not live-tested.** Every address here is from static analysis of the
  pre-Anniversary movies install's `client.dll`, checked by the verifier above.
  It has not yet been run against a game.

---

## 6. The other route, not taken here

Because the TAB presses are ordinary `ConsoleCommand` frames (§1), the patcher
could blank them at patch time instead. The command field is a fixed
`char command[64]`, so overwriting it is a pure in-place byte edit: no frame
added or removed, and therefore **no frame ordinals shift** — the +1 hazard
`CLAUDE.md` warns about does not apply.

That route needs no companion DLL at all, and is a targeted instance of #30
(strip pre-existing console commands from a source demo). It is worth doing as
well, not instead: it bakes the decision in at patch time, where this one is
live-toggleable while reviewing a demo.
