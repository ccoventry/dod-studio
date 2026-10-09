# The crosshair: hiding it, and matching it while spectating

`dodstudio_hide_crosshair` and the crosshair half of `dodstudio_spec_match_pov`.
Split out of `docs/goldsrc_hud_suppression.md` (which holds the shared
patch-instead-of-edit-a-file pattern and the per-frame re-apply loop, there §4
and §1). Same method and verification: offline analysis of DoD 1.3's
`client.dll`, checked by `goldsrc-hooks/tools/verify_voice_crosshair_offsets.py`
and `verify_spectator_crosshair_offsets.py`.

§2 is the odd one out: it puts something *back* rather than taking it away. It
lives beside §1 because it patches the same function, and because the reason it
is needed at all is the fork §1 had to map.

---

## 1. `dodstudio_hide_crosshair`

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
`dodstudio_hudelement` command in #265 (shipped as `dodstudio_hide_hudelement`, see `docs/goldsrc_hud_elements.md`) is still worth having; this is not it and
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

## 2. The spectator crosshair (`dodstudio_spec_match_pov`)

> This had a cvar of its own, `dodstudio_match_pov_crosshair`, until
> 2026-10-01. It is now one of the things `dodstudio_spec_match_pov` turns
> on. Where the text below says `dodstudio_match_pov_crosshair`, read
> `dodstudio_spec_match_pov`.

The other half of §1's finding. Mapping the fork to prove the hide covered both
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

### When it is hidden

The spectator branch draws its crosshair whenever the camera is in a player's
eyes. The player's own view does not: `ShouldDrawCrossHair` (`client+0x2d0e0`)
hides it while the gun is lowered and for weapons that have none. With
`dodstudio_spec_match_pov 1` the spectated view hides it in the same states
(#310).

**The reference is the game being played, not a POV demo.** The two differ.
Recording POV demos to frames and looking for the crosshair in each one
(about 13,000 frames, six demos) gives:

| state | playing live | in a POV demo | spectated, with the switch |
| --- | --- | --- | --- |
| sprint key held and moving | hidden | hidden, to the frame | hidden |
| in the air after a jump (not a plain fall) | hidden | **shown** (2 of 35) | hidden |
| going prone, getting up | hidden | hidden 1.53s from the start | hidden 1.5s |
| prone and moving | hidden | hidden, to the frame | hidden |
| on a ladder | hidden | hidden | hidden |
| knife, spade, Springfield, scoped K98, scoped Enfield | hidden | hidden | hidden |
| MG42, MG34, .30 cal not deployed | hidden | hidden, back the instant it deploys | hidden |
| dead | hidden | hidden | hidden |
| 0.5s after drawing a weapon (0.8s K43, 0.68s Colt, 1s Webley and rockets) | hidden | **shown** (4 of 706 frames hidden) | hidden |
| switching to or from a grenade | shown | shown | shown |
| reloading | hidden | **shown** (0 of 75) | hidden |
| 1.6s after a bolt rifle's shot | hidden | **shown** (0 of 132) | hidden |

The four that differ are driven by the player's own client predicting his
weapon and his jump (`flBoltHideXHair`, `g_iinjump`), and none of that runs
while a demo plays. Shown both, the user chose live play (2026-10-01). Until
then this matched the POV demo; that version is #556's second commit.

The switch times come from dod13-client's `dlls/wpn_shared/*.cpp`: most
weapons deploy through `DefaultDeploy` (0.5s), and the `TimedDeploy` ones set
their own. Switching *from* a grenade would start the timer by the code, but
the user saw no hide either way when playing, so neither direction starts it.

Every state it does hide is read from what an HLTV demo carries for each
player (`anim_fix/crosshair_rule.rs`):

| POV's test | read from |
| --- | --- |
| sprint key and a move key | gait `dod_sprint`. On the same player in his POV demo and the HLTV demo of that half, the HLTV gait matched his sprint key 99.7% of the time |
| jump | body `jump`, from take-off until landing |
| weapon switch | the viewmodel changing, plus the weapon's switch time |
| reload | the body playing a `*_reload` sequence |
| bolt cycle | a shot from the K98 or Enfield, then 1.6s |
| prone transition | 1.5s from the body entering `get_down` / `get_up` (those run 1.3s and 2.0s, so neither length is the answer) |
| prone and a move key | gait `prone_forward`. Gait `dod_crawl` is the *crouched* walk and hides nothing |
| ladder | `movetype` 5 |
| weapon | the third-person model held |
| machine gun deployed | body `sandbag_*` / `bipod_*` |

Where it is not exact: crawling can read up to half a second long (the gait
stays `prone_forward` while the player slides to a stop); fully underwater
and the scoped FG42 while zoomed are left out, because nothing replicated says
so reliably.

The hide costs no new hook. The spectator draw (`client+0x2d1f0`) already has
a gate of its own, 13 bytes just before the rect: it skips the crosshair while
the view is zoomed (`0 < fov < 90`). While the player's own view would have no
crosshair, those 13 bytes become a jump to the same exit; when the state ends,
the stock bytes go back.

An empty rect does **not** work, and was the first attempt: the engine takes a
rect with no size to mean the whole sprite, so all sixteen tiles of
`customXHair.spr` appeared below and right of the screen centre.

To check it against a recording: `goldsrc-hooks/tools/crosshair_frames.py`
finds the crosshair in recorded frames, and compares them with the hook's own
trail (`hook`) or with a POV demo's state from
`analysis/examples/crosshair_pov_probe.rs` (`pov`). It looks at the screen
centre only, which is how the whole-sheet draw got past it at first; look at a
frame too. On `monday-wsod25_r07_m1_h1_hltv`, in a stretch where the spectated
player sprints, goes prone, crawls and gets up, the frames and the trail agree
on 884 of 919 (pre-Anniversary) and 929 of 967 (25th Anniversary); every miss
is the one frame at a change.

### It loses to §1, by construction

`dodstudio_hide_crosshair` stubs `Draw`'s prologue, so neither branch runs. This
patches instructions *inside* a function that is then never reached, so hiding
wins with no interlock written anywhere.

### What this does not answer

#219 asks why the POV and HLTV first-person crosshairs differ. §1 found half of
it — the spectator branch never reads the `crosshair` cvar. This is the other
half: it never reads `cl_xhair_style` either. The third was *when* it draws,
answered above. Still open: accuracy spread, which the POV crosshair shows and
a spectator cannot know (it comes from the player's own prediction).

