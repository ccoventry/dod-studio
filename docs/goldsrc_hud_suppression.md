# Silencing and hiding things DoD draws, without editing game files

`dodtools_mute_voice_commands` and `dodtools_hide_crosshair`, and why each needed a
patch rather than a setting. Companion to `docs/goldsrc_scoreboard.md`, which
covers `dodtools_hide_scoreboard`.

Everything here is from offline analysis of DoD 1.3's `client.dll` (`pefile` +
`capstone`, the house method in `docs/goldsrc_client_dll_internals.md` §10),
checked by `goldsrc-hooks/tools/verify_voice_crosshair_offsets.py` and
`verify_spectator_crosshair_offsets.py`.

§6 is the odd one out: it puts something *back* rather than taking it away. It
lives here because it patches the same function, and because the reason it is
needed at all is the fork §3 had to map.

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

All four settings (including `dodtools_hide_scoreboard`) are handed to their
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

For §6 the same loop earns its keep twice over: polling is also how it notices
`cl_xhair_style` being changed under it, with no cvar callback needed.

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

---

## 6. `dodtools_match_pov_crosshair`

The other half of §3's finding. Mapping the fork to prove the hide covered both
crosshairs also showed *why* they never look alike:

```text
mode 0        POV. Reads cl_xhair_style. Non-zero -> client+0x2ced0, which
              draws a 64x64 tile out of customXHair.spr. Zero -> the HUD
              sprite list's crosshair at client+0x2cda0.
mode 3 or 4   client+0x2d1f0. Hardcodes crosshairs.spr and a 24x24 rect, and
              reads no cvar at all.
```

So a custom crosshair set up for play is simply absent while spectating, and
what you get instead is a 24x24 tile of a 128x128 sprite — 576 pixels to make a
crosshair out of.

### Nothing needs loading

`CHudDoDCrossHair::VidInit` loads **both** sprites unconditionally:

```asm
client+0x2cc82  call pfnSPR_Load     ; "sprites/crosshairs.spr"
client+0x2cc88  mov [esi+0x60], eax
client+0x2cca0  call pfnSPR_Load     ; "sprites/customXHair.spr"
client+0x2cca6  mov [esi+0x74], eax
```

The custom sprite's handle is already on the object, unused on this path.

### Five fields in one 57-byte span

```asm
client+0x2d205  mov dword [ecx+0x64], 0x18   ; left   = 24
client+0x2d20c  mov dword [ecx+0x6c], 0      ; top    = 0
client+0x2d213  mov dword [ecx+0x68], 0x30   ; right  = 48
client+0x2d21a  mov dword [ecx+0x70], 0x18   ; bottom = 24
...
client+0x2d23b  mov eax, [ecx+0x60]          ; the sprite handle
```

`0x60` becomes `0x74`, and the four immediates become the selected tile.
Swapping the handle alone would sample a 24x24 corner out of a 256x256 sprite;
changing the rect alone would sample off the end of a 128x128 one. One change,
one patch, one signature.

### The grid is DoD's, not ours

`client+0x2ced0` — the POV path — computes its rect as:

```text
if (style > 16) style = 16;
style--;
col = style % 4;  row = style / 4;
left = col << 6;  right  = (col + 1) << 6;
top  = row << 6;  bottom = (row + 1) << 6;
```

A 4x4 grid of 64x64 tiles, which is exactly how the shipped `customXHair.spr`
is laid out (256x256, one frame, sixteen crosshairs — read straight out of the
`.spr`). `spectator_crosshair::tile_rect` reproduces that arithmetic rather
than inventing a layout, so the spectator view gets the *same* tile the player
sees, whatever they set.

`cl_xhair_style 0` is not tile 0: zero sends the POV path to a different
function entirely, so there is no custom crosshair to match and this leaves the
stock rect alone rather than guessing.

### It loses to §3, by construction

`dodtools_hide_crosshair` stubs `Draw`'s prologue, so neither branch runs. This
patches instructions *inside* a function that is then never reached, so hiding
wins with no interlock written anywhere.

### What this does not answer

#219 asks why the POV and HLTV first-person crosshairs differ. §3 found half of
it — the spectator branch never reads the `crosshair` cvar. This is the other
half: it never reads `cl_xhair_style` either. Whether anything *else* differs
between the two views is still open.

---

## 7. `dodtools_hide_hudelement` -- the general case

Sections 2, 3 and 6 each patch one function for one purpose, and section 3's
own module doc says why it is not this: patching a function needs no vftable
address and reverts to bytes its own signature already proved were there. Fine
for one element; not a plan for twelve.

`CHud::Redraw` walks a linked list and calls each element's **vftable slot 3**,
`Draw`. `CHudBase::Draw` is `xor eax, eax; ret 4` -- a complete no-op with the
right calling convention, which five of the twenty-two elements already use
unchanged because they never override it.

So hiding any element is **writing that one address into its slot 3**. No
signature, no detour, no stub, no relocation, nothing left mid-instruction.
Showing it again is writing back the dword that was there.

### Why the vftable and not `m_iFlags`

Clearing bit 0 of an element's `m_iFlags` is per-*instance*, which sounds
better. The game writes that field itself from `Init`, `VidInit`, `Reset` and
in some cases `Draw`, so it would have to be re-applied against the game's own
writes. A vftable is written once by the constructor and never touched again.

### Fixed RVAs, verified by name

This is the one module here built on fixed offsets rather than a signature,
because a vftable has no code to sign. What replaces the signature is better
than one: DoD's `client.dll` ships **MSVC RTTI**, so `vftable[-1]` is a
complete object locator whose type descriptor carries the class' decorated
name. Every entry in the table names the class it expects
(`.?AVCHudSayText@@`), and nothing is written until the loaded module agrees.
A wrong build fails loudly, by name.

`goldsrc-hooks/tools/verify_hudelements.py` checks the same thing offline, and
adds the check the DLL cannot make for itself: **completeness**. It finds every
class in the image whose `Init` calls `CHud::AddHudElem` and which overrides
`Draw`, and fails if any of them is missing from the table (or a documented
exclusion). On the shipped `client.dll` that is 22 registering classes, 17 of
which draw -- 12 in the table, plus five deliberately excluded below.

### The five that are not listed

`CClientEnvModel`, `CHudTextMessage`, `CParticleShooter`, `CVoiceStatusHud` and
`CWeatherManager` do not override `Draw`. They are on the list to receive user
messages and to be ticked, not to draw, so hiding them is already true and
offering it would only invite the question of why it did nothing.

`CVoiceStatusHud` is also the one element with **two** vftables (`+0xabb48` and
`+0xabb24`), because it inherits from both `IVoiceHud` and `CHudBase`; only the
second is the element's. Worth knowing before anyone adds an entry.

### The five that override `Draw` and still aren't listed

Each of these registers itself and overrides `Draw`, so each would fail the
completeness check above like a genuine miss unless named as an exception.
None of them is a miss, but for two different reasons.

`CHudAmmo` draws (the ammo counter and the weapon-select menu) for real:
disassembly of `client+0x28b00` (`CHudAmmo::Draw`, 2604 bytes) shows every
`FillRGBA`/`SPR_Draw` pair in the function landing after one of its four
`CHud::ShouldDraw(3)` calls, and nothing drawing before the first one. The
stock `cl_hud_ammo` cvar already hides all of it -- unlike
`crosshair`/`r_drawentities`/`cl_lw`, `cl_hud_ammo` is not one of the cvars
`CHud::Redraw` forces back every frame (§3 above), so setting it from a config
actually sticks. `objectives` and `icons` below were checked the same way and
kept, because both draw something *before* their own `ShouldDraw` gate that no
stock cvar reaches.

The other four don't draw anything at all, in this build, regardless of any
cvar or hook:

- `CHudDoDMap::Draw` (`client+0x2e560`) is `mov eax, 1; ret 4` -- eight bytes,
  no calls. The overview map is rendered some other way entirely; not yet
  found, plausibly VGUI2 like the scoreboard.
- `CMortarHud::Draw` (`client+0x3e720`) calls one `gHUD` helper that checks a
  flag byte and an observer sub-mode value, then returns a plain boolean.
  Neither function contains a `FillRGBA` or `SPR_Draw` call. There is no
  mortar aiming HUD in this build to hide.
- `CHudSpectator::Draw` (`client+0x38000`) is 45 bytes ending in a real
  `ret 4` immediately followed, with no padding, by an unrelated function --
  a naive linear disassembly scan folds the two together and badly overstates
  the size, so measure carefully if re-checking this one. The real function
  checks observer mode and conditionally calls a method on what looks like a
  VGUI2 interface pointer, plausibly telling a panel to hide, but never draws.
- `CHudScope::Draw` (`client+0x46590`, 22 bytes) reads one flag and one
  observer-mode global, then unconditionally returns 1. The actual scope
  vignette is a `ScreenFade` engine call inside `CHudScope::Think` (vftable
  slot 4, not 3), gated on the *local* player's own current weapon -- never
  populated while spectating, live-confirmed: no scope overlay appears in a
  demo, matching the disassembly exactly.

Writing `CHudBase::Draw` over a function that already draws nothing changes
nothing observable, so offering these four would only mislead.

### `all 1` is refused

`all 0` shows everything again, which is what a way out looks like. `all 1` is
refused on purpose: it would hide `CHudMenu` too, and a session that cannot see
the class menu is a support question rather than a feature.

### What it reaches that nothing else did

Chat, the kill feed, the status bar under the crosshair, the team and class
menus, the tram controls, the VGUI2 print panel, the objective icons, and
`CHudDodIcons` -- which owns **both** the MG-deploy icon (#288) and the
capture-area icon (#289), along with blood and bandage. Those two were filed
separately because the icons look unrelated; one element draws all four, so
they are one switch, and separating them would mean patching inside a
1507-byte `Draw`.

The objective-icons element (`objectives`) is coarser than its name suggests
too: `CObjectiveIcons::Draw` also owns the reinforcement-wave countdown clock
(a separate, internally-gated block inside the same function, distinct from
`CHudDodIcons`'s own reinforcement icon), so hiding `objectives` hides that
clock along with the flags/capture-progress row it's named for -- there is no
way to keep one and drop the other without patching inside the function.

It does **not** reach the VGUI2 spectator bars (§5) or the auto-help panel
(#286). Those are not HUD elements and are not on this list. It also does not
reach the overview map, a mortar aiming HUD, or the sniper scope vignette --
see the five exclusions above, none of which turned out to be drawn by any
`Draw` override at all.

