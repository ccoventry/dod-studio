# `dodstudio_hide_hudelement` and the CHudDoD* element notes

Split out of `docs/goldsrc_hud_suppression.md` (the shared pattern, voice
muting and the per-frame re-apply loop stay there; the crosshair is in
`docs/goldsrc_crosshair.md`). References below point at those
two docs (the shipped spectator bars hide is
`docs/goldsrc_spectator_bars.md`).

Same method as the rest: offline `client.dll` analysis, plus
`goldsrc-hooks/tools/verify_hudelements.py`.

---

## `dodstudio_hide_hudelement` -- the general case

The voice mute, the crosshair hide and the spectator crosshair each patch one function for one purpose, and the crosshair hide's
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
which draw -- 10 in the table, plus seven deliberately excluded below.

### The five that are not listed

`CClientEnvModel`, `CHudTextMessage`, `CParticleShooter`, `CVoiceStatusHud` and
`CWeatherManager` do not override `Draw`. They are on the list to receive user
messages and to be ticked, not to draw, so hiding them is already true and
offering it would only invite the question of why it did nothing.

`CVoiceStatusHud` is also the one element with **two** vftables (`+0xabb48` and
`+0xabb24`), because it inherits from both `IVoiceHud` and `CHudBase`; only the
second is the element's. Worth knowing before anyone adds an entry.

### The seven that override `Draw` and still aren't listed

Each of these registers itself and overrides `Draw`, so each would fail the
completeness check above like a genuine miss unless named as an exception.
None of them is a miss, but for three different reasons.

`CHudAmmo` draws (the ammo counter and the weapon-select menu) for real:
disassembly of `client+0x28b00` (`CHudAmmo::Draw`, 2604 bytes) shows every
`FillRGBA`/`SPR_Draw` pair in the function landing after one of its four
`CHud::ShouldDraw(3)` calls, and nothing drawing before the first one. The
stock `cl_hud_ammo` cvar already hides all of it -- unlike
`crosshair`/`r_drawentities`/`cl_lw`, `cl_hud_ammo` is not one of the cvars
`CHud::Redraw` forces back every frame (`goldsrc_crosshair.md` §1), so setting it from a config
actually sticks. `objectives` and `icons` below were checked the same way and
kept, because both draw something *before* their own `ShouldDraw` gate that no
stock cvar reaches.

The other five don't draw anything at all, in this build, regardless of any
cvar or hook:

- `CHudDoDCommon::Draw` (`client+0x2c6f0`, 412 bytes) is not the "shared HUD
  backdrop" the name implies. No `FillRGBA`/`SPR_Draw` call anywhere in it --
  instead it fires `+attack;wait;-attack` four seconds after the player you're
  spectating dies (cycling the observer target), copies two per-team gameplay
  flags (paratrooper mode, infinite lives) other elements read, and clears the
  sniper scope after a server-driven camera-view event ends. Confirmed against
  the real DoD 1.3 client source, `whamemer/dod13-client`'s
  `cl_dll/dod_common.cpp` -- `CHudDoDCommon::Draw` there matches the
  disassembly instruction for instruction. Live testing this element bore
  that out: nothing observable changed either way, because there was nothing
  to see.
- `CHudDoDMap::Draw` (`client+0x2e560`) is `mov eax, 1; ret 4` -- eight bytes,
  no calls. The overview map is rendered some other way entirely; not yet
  found, plausibly VGUI2 like the scoreboard. *(Update: the map's placement is
  cached geometry in `.data`, not drawn by a `Draw` override, and
  `dodstudio_overviewmap` (`goldsrc-hooks/src/overview_map.rs`, see
  `docs/goldsrc_client_dll_survey.md` §5) moves and resizes it. Hiding it
  outright is still not done.)*
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
nothing observable, so offering these five would only mislead.

One more is excluded for a third reason -- it draws for real, but nothing has
ever been seen to make it draw:

- `CHudStatusIcons::Draw` (`client+0x471c0`) loops over four icon slots and
  calls `SPR_DrawAdditive` for any that hold a sprite handle -- a genuine
  draw, unlike the five above. What populates a slot is the `StatusIcon` user
  message (`(enable byte, icon-name string, [r,g,b] if enabling)`, confirmed
  against `client.dll` itself via `MsgFunc_StatusIcon`/`EnableIcon`, not just
  source). That message never appeared in 661 real demos checked: the local
  test library, four demos purpose-recorded trying to trigger it, and 622
  more from a full install scan. The one lead disassembly turned up --
  `EnableIcon`'s `strstr(name, "grenade")` hack that plays `weapons/timer.wav`,
  suggesting a grenade-cook countdown icon -- doesn't hold up: that sound file
  doesn't exist in any install, and `sprites/hud.txt` has no sprite registered
  under the plain name `"grenade"` for `GetSpriteIndex` to resolve against.
  Excluded because there is nothing left to test it against, not because it
  is proven dead the way the five above are.

### `vgui2print` is narrower than its name suggests

`CHudVGUI2Print` has three real callers, but only one goes through the
vtable slot this element hides:

- `CHudDoDCommon` (key `Dod_mg_reload` -- "Deploy your machine gun to
  Reload!") calls a *queueing* method (`client+0x3a990`) that only writes
  text/position/color/expiry into `CHudVGUI2Print`'s own fields. The pixels
  are drawn later, when `CHudVGUI2Print::Draw` itself runs and reads that
  state -- the vtable-dispatched path this element's hide actually
  intercepts. Live-confirmed.
- `CObjectiveIcons` (key `clan_warmup_mode` -- "Warmup Mode") calls a
  *drawing* method (`client+0x3a3f0`) that renders immediately via a shared
  low-level helper (`client+0x3a4b0`) at a fixed address, never touching
  `CHudVGUI2Print::Draw` or its vtable slot. Hiding `vgui2print` does not
  hide this banner.
- `CHudMenu::Draw` calls that same low-level helper directly too, for its
  own rich-text menu list (`\`-prefixed color/newline escape codes,
  word-wrapped one call per word). Also untouched by hiding `vgui2print` --
  `menu`'s own vftable hide is what stops this, by not letting
  `CHudMenu::Draw` run at all.

So `vgui2print` only ever hides queued, timed prompts like the MG-reload
message. Anything drawn immediately through the shared helper is a different
code path this element's vtable slot cannot see.

### `all 1` is refused

`all 0` shows everything again, which is what a way out looks like. `all 1` is
refused on purpose: it would hide `CHudMenu` too, and a session that cannot see
the class menu is a support question rather than a feature.

### What it reaches that nothing else did

Chat, the kill feed, the status bar under the crosshair, the team and class
menus, the tram controls, the objective icons, and `CHudDodIcons` -- which
owns **both** the MG-deploy icon (#288) and the capture-area icon (#289),
along with blood and bandage. Those two were filed
separately because the icons look unrelated; one element draws all four, so
they are one switch, and separating them would mean patching inside a
1507-byte `Draw`.

The objective-icons element (`objectives`) is coarser than its name suggests
too: `CObjectiveIcons::Draw` also owns the reinforcement-wave countdown clock
(a separate, internally-gated block inside the same function, distinct from
`CHudDodIcons`'s own reinforcement icon), so hiding `objectives` hides that
clock along with the flags/capture-progress row it's named for -- there is no
way to keep one and drop the other without patching inside the function.

It does **not** reach the VGUI2 spectator bars (`goldsrc_spectator_bars.md`) or the auto-help panel
(#286). Those are not HUD elements and are not on this list. It also does not
reach the overview map, a mortar aiming HUD, or the sniper scope vignette --
see the five exclusions above, none of which turned out to be drawn by any
`Draw` override at all.

