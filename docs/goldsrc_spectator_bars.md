# The spectator top/bottom bars: why one `.res` edit sticks and the other doesn't

R&D into a `dodstudio_hide_spectator_bars`-style command, prompted by the user
asking why HLAE's `mirv_movie_hidepanels`/`mirv_disable_specmenu` don't help
(`mirv_movie_hidepanels` only hides panels *from the capture*, leaving them
on screen — confirmed from the advancedfx wiki, not from memory; the wiki
carries no implementation detail beyond that one sentence. `mirv_disable_specmenu`'s
own "Supported modifications" list is `tfc`/`valve` only — DoD isn't on it).

Not finished. This is a progress report and a narrower set of open questions
than `docs/goldsrc_client_dll_survey.md` §10 left — not a working patch. See
§5 for exactly what's still missing before this is safe to implement.

---

## 1. The two `.res` files, and the asymmetry that started this

`resource/ui/Spectator.res` (`"Resource/UI/SpectatorGUI.res"`) declares a
`TopBar` panel, a `BottomBar` *frame*, and DoD's own score/timer labels
(`AlliesScoreLabel`, `AxisScoreLabel`, `ReinforcementsLabel`, `timerlabel`).
`resource/ui/BottomSpectator.res` (`"Resource/UI/BottomSpectatorGUI.res"`) is
a separate file: one `bottombar` frame containing the three mode/player/view
comboboxes and the prev/next buttons — the actual interactive control bar.

The user's own finding: editing `Spectator.res`'s `visible`/`enabled` fields
does hide the top bar. Editing `BottomSpectator.res` the same way does not
touch the bottom bar at all. That asymmetry is the whole reason this file
exists — something is re-asserting the bottom bar's visibility that the top
bar either doesn't have or doesn't hit the same way.

---

## 2. `client.dll` has real, RTTI-confirmed classes for both — and both are VGUI2, not VGUI1

`goldsrc-hooks/tools/survey_client_dll.py`'s RTTI walk (§"RTTI" in that
script) finds two type descriptors and two matching vtable pairs for the top
bar, both compiled into `client.dll` itself (not merely referenced):

```
.?AVCSpectatorGUI@@       -- stock HL SDK base class
.?AVCDoDSpectatorGUI@@    -- DoD's own subclass
```

Each has **two** vtables (multiple inheritance — a primary at offset 0, a
second interface at offset `+0x10c`):

```
CDoDSpectatorGUI vtable A: +0xaab84   (primary, 40 slots)
CDoDSpectatorGUI vtable B: +0xaab3c   (secondary, ISpectatorInterface, object+0x10c)
```

The bottom bar has its own, separate RTTI class, confirmed the same way:

```
.?AVCBottomBar@@                    -- bottom bar container, one vtable, +0xb51b4
.?AVCommandComboBox@CBottomBar@@    -- its mode/player/view comboboxes, +0xb4e3c
```

**§3 below found the earlier "VGUI1" framing of this section wrong.** Both
`CDoDSpectatorGUI` and `CBottomBar` are concrete leaves of **`vgui2::Frame`**
— the newer VGUI2 UI system, not the older `vgui.dll` VGUI1 one their RTTI
names suggest. Confirmed by diffing each class' vtable, slot for slot,
against `client.dll`'s own statically-linked `vgui2::Panel`/`vgui2::Frame`
RTTI (`.?AVPanel@vgui2@@` at `+0xad97c`, `.?AVFrame@vgui2@@` at `+0xade94`):
`CDoDSpectatorGUI`'s primary vtable matches `Frame@vgui2`'s on 35 of 40 slots
exactly (identical function addresses — inherited, not overridden);
`CBottomBar`'s single vtable matches on 34 of 40. Both diverge from `Frame`
in roughly the same handful of slots (27-30, 36ish) — the small set of
methods each subclass genuinely customizes (layout/command handling, not
touched here).

This also resolves where the `vgui2::` classes come from: `vgui2.dll` itself
carries almost no RTTI (`VPanel`/`IPanel` interface wrappers only) and
exports exactly one symbol — the real `vgui2::Panel`/`Frame`/`ComboBox`
*implementations* are statically linked into `client.dll` directly. One of
their RTTI descriptors leaked Valve's actual internal build path:

```
.?AVCaptionGripPanel@?%C:\buildslave\goldsrc_win32\build\GoldSrc\vgui2\controls\Frame.cpp303372893@@
```

`GoldSrc\vgui2\controls` is a real buildslave path, not a DoD-authored
reimplementation — hard evidence for the "backported from a Source-era vgui2
during the 2013 Steampipe update" hypothesis raised for this investigation:
Valve maintained (and still built, as of whatever `client.dll` this survey
targets) a dedicated GoldSrc port of the newer vgui2 controls library.

`CDoDSpectatorGUI` is **not dead code**. Its constructor is a real, reachable
function:

```
+0x1da20   CDoDSpectatorGUI::CDoDSpectatorGUI (ctor)
```

called from inside a larger panel-setup routine, via the standard MSVC
`new`+ctor pair:

```
+0x1f045   push 0x150            ; sizeof(CDoDSpectatorGUI) == 336
+0x1f04a   call operator_new     ; +0x1987e3b
+0x1f063   mov ecx, eax
+0x1f065   call CDoDSpectatorGUI::ctor   ; +0x1da20
+0x1f074   mov [esi+0x740], eax          ; stored as a member of some
                                          ; larger container object
```

Three sibling `new`+ctor pairs sit right next to this one, storing into the
*same* container (`esi`) at offsets `+0x734`, `+0x73c`, `+0x744` — plausibly
`CBottomBar`'s own instance is one of these three, constructed alongside the
top bar's `CDoDSpectatorGUI` rather than being a child of it. **Not yet
identified which offset is which, or `CBottomBar`'s own constructor call
site** — see §5.

---

## 3. Both bars share the exact same `Frame::SetVisible` — there is no DoD override to hook

The doc originally (before this correction) read `vgui.dll` — Valve's
**separate VGUI1** runtime — for a `setVisible` slot number, and applied that
slot number to these VGUI2 classes. That doesn't transfer: VGUI1 and VGUI2
are unrelated class hierarchies with unrelated method orders, and per §2
neither bar is VGUI1 in the first place. Re-derived from scratch against
`vgui2::Panel`/`Frame`'s own vtables:

- Slot 9 (`+0x62060` in both classes) is **not** `setVisible`. Disassembly
  shows a thiscall taking two stack ints (`ret 8`) that walks child panels
  comparing anchor-pin values and repositioning them — shaped like
  `OnSizeChanged(wide, tall)`, not a one-bool setter. It's also inherited
  unchanged from `Frame` in both classes, not a DoD override of anything.
- Slot 8 is the real `setVisible`-shaped candidate: `vgui2::Panel`'s own
  slot 8 is a 3-byte no-op stub (`ret 4`), and `Frame` replaces it with a
  real, 77-byte, one-bool-arg (`ret 4`) function that reads an animation
  controller and, when present, forwards the bool through it — the shape
  Source-era `Panel::SetVisible`/`Frame::SetVisible` actually has (drive
  fade/close animations, not just flip a bit). **Both `CDoDSpectatorGUI` and
  `CBottomBar` inherit this exact same address unmodified** — neither
  overrides it.

Practically: the "`CDoDSpectatorGUI` overrides `setVisible` to propagate
visibility to children, which is why poking `.res` isn't the whole story"
theory from the earlier version of this section **is retracted** — there is
no such override. Whatever is putting the bottom bar back after a `.res`
edit is not a DoD-authored `SetVisible` hook; more likely something else
calls `SetVisible(true)` on it after construction (a mode-change handler, a
per-frame HUD update, or the `.res` scheme simply never being consulted for
this particular object to begin with). Not yet traced — see §5.

One methodological trap found along the way, worth keeping for future
disassembly in this compilation unit: `client.dll`'s vgui2-controls code is
**incrementally linked** — several vtable slots (slot 8 among them) don't
point at a real function body, they point at a 5-byte `jmp rel32` + `nop`
padding stub (an ILT thunk). Feeding a thunk address straight into
`function_end()`'s linear-scan heuristic produces garbage: it decodes through
the thunk, into the padding, and into the *next*, unrelated thunk, reporting
a bogus size built from three different functions concatenated. Follow the
`jmp` target first, then disassemble from there.

---

## 4. The `+0x1a9d564` global from the existing survey is *not* the panel — corrects §10

`docs/goldsrc_client_dll_survey.md` §10 calls `+0x1a9d564` "a cached VGUI2
interface pointer" that `CHudSpectator::Draw`'s 45-byte body reads before
asking a panel whether to show itself. True, but it undersells how far that
pointer actually reaches: it is referenced **141 times**, spanning nearly
the entire `.text` section (`+0x1c5f1` to `+0x874f3`), through vtable-call
offsets as large as `+0x368` (slot 218+). No 30-or-46-slot `Panel`/`Frame`
subclass has that many virtuals — this is a much bigger, shared VGUI2-era
root interface (surface/panel-tree scale), not `CDoDSpectatorGUI` itself and
not specific to the spectator bars. `CHudSpectator::Draw`'s use of it (only
in the `mode == 0` edge case, per §10) is one caller among many, not evidence
that this global *is* the bar.

Practical effect: the bars are not reachable through that global the way
`docs/goldsrc_client_dll_survey.md` §10 implied might be worth chasing. The
right object to chase is the `CDoDSpectatorGUI` instance itself (§2), reached
through whatever fixed global holds its *container* object — not found yet.

---

## 5. What's actually missing before this is safe to implement

1. **The container object's own address.** `CDoDSpectatorGUI`'s `this` is
   stored at `[container+0x740]`; the container itself is not yet resolved to
   a fixed global (the way `CHUD_SPECTATOR = 0x115da8` was for `CHudSpectator`
   in the existing survey). Without that, there's no way to read the panel
   pointer at runtime the way `commands.rs`'s other patches read a known
   global.
2. **Which of `+0x734`/`+0x73c`/`+0x740`/`+0x744` is the bottom bar**, and
   `CBottomBar`'s own constructor call site — not yet traced. `CBottomBar`'s
   RTTI and vtable are confirmed real (§2), but which sibling `new`+ctor pair
   builds it, and where *its* `this` gets stored, is still unknown.
3. **What actually calls `SetVisible` on these objects, and when.** §3
   retracted the "DoD overrides `setVisible`" theory — both bars use the
   plain, shared, inherited `Frame::SetVisible` (real body at `+0x61f70`,
   reached through a `jmp`-thunk at the vtable's `+0x61420` slot). Since
   there's no override to read for DoD-specific behavior, the actual
   "something re-asserts the bottom bar's visibility" mechanism has to be
   found elsewhere — most likely by finding *callers* of that vtable slot
   (virtual dispatch, so not found by a plain `calls_to()` on the function
   address — needs a scan for `call dword ptr [reg+0x20]`-shaped indirect
   calls whose `reg` is provably a `CBottomBar`/`Frame` instance), or by
   checking whether `BottomSpectator.res`'s values are read into this object
   at all during construction.
4. **A live check that patching `setVisible(false)` doesn't crash.**
   Everything above is from `pefile`/`capstone` alone, no running game.
   Because slot 8 is the stock, shared `Frame::SetVisible` (not
   DoD-specific), calling it externally is lower-risk than the retracted
   per-class-override theory implied — but still untested outside whatever
   normally drives it. This is exactly the class of mistake the project's
   `tools/verify_*.py` scripts exist to catch before a patch ships, and
   there's no `verify_spectator_bars_offsets.py` yet because there's no
   confirmed offset to verify.

Once (1)-(3) are resolved, the shape of the fix is still likely to hook
`SetVisible` (or drive it directly, like `crosshair.rs`/`scoreboard.rs`
already do for other `CHudBase`-external state) rather than trying to control
the `.res` file — but unlike the earlier draft of this section, that's no
longer backed by a DoD-specific override to hook; it would mean hooking the
*shared, stock* `Frame::SetVisible`, which every other VGUI2 `Frame` in this
client also uses, so any hook needs to filter by `this` (a `CBottomBar`/
`CDoDSpectatorGUI` instance) rather than assuming the function itself is
bar-specific.

---

## Attempted and parked: `dodstudio_hide_spectator_bars`

**Status (2026-09-21): parked.** Not wired into the crate (`goldsrc-hooks/src/spectator_bars.rs`
exists on disk, but `lib.rs` doesn't declare it as a `mod`) after two live
tests confirmed the whole `SetVisible`-based approach is a dead end -- see
below. Kept on disk rather than deleted: the class hierarchy, ownership
chain and vtable layout it establishes are all still correct and would save
real time if picked up again, even though the mechanism itself didn't work.
Every open question in §5 got resolved anyway by continuing the same static
analysis (this section describes what that produced, before the live test
that ruled it out):

1. **The container.** `gViewPort` -- the same global `scoreboard.rs`'s
   `+showscores` patch already reads (`+0x19d564`) -- holds `DoDViewport*`.
   `DoDViewport::VidInit` (found via its own vtable slot 2, RTTI-confirmed
   `.?AVDoDViewport@@`) builds five sibling members with the standard MSVC
   `push sizeof; call operator_new; call ctor; mov [container+off], eax`
   pattern; the fourth is `CDoDSpectatorGUI`, stored at `+0x740`
   (`sizeof(CDoDSpectatorGUI) == 0x150`, matching the RTTI-derived vtable's
   own construction call at `+0x1f065`).
2. **The bottom bar's container.** Not one of those five siblings after all
   -- that was this doc's own earlier, wrong guess. `CBottomBar`'s real
   constructor (`client+0x81df0`) has exactly one caller in the whole image:
   `client+0x82c66`, inside `CSpectatorGUI::CSpectatorGUI`
   (`client+0x82b50`, RTTI-confirmed `.?AVCSpectatorGUI@@`), which stores it
   at `+0x114`. `CSpectatorGUI` is `CDoDSpectatorGUI`'s own base class --
   confirmed directly, not inferred: `CDoDSpectatorGUI::CDoDSpectatorGUI`
   (`+0x1da20`) calls `CSpectatorGUI::CSpectatorGUI` at `+0x1da42` as its
   base-class-init step. Single, non-virtual inheritance places a base
   subobject's members at the same offsets off the derived `this`, so
   `CBottomBar* = *(CDoDSpectatorGUI* + 0x114)` needs no separate container
   pointer at all.
3. **Both bars are VGUI2, not VGUI1.** Diffing each class' vtable slot-for-
   slot against `client.dll`'s own statically-linked `vgui2::Panel`/`Frame`
   RTTI showed `CDoDSpectatorGUI` matches `Frame` on 35/40 slots,
   `CBottomBar` on 34/40 -- both are concrete `vgui2::Frame` leaves. One RTTI
   descriptor even leaked Valve's real build path,
   `C:\buildslave\goldsrc_win32\build\GoldSrc\vgui2\controls\Frame.cpp` --
   independent evidence for the "backported during the 2013 Steampipe
   update" hypothesis raised for this investigation.
4. **No DoD override to hook.** Re-deriving `SetVisible`'s slot against
   VGUI2 ground truth (not `vgui.dll`'s unrelated VGUI1 layout, §3's original
   mistake) put it at slot 8, not 9: `Panel@vgui2`'s own slot 8 is a 3-byte
   no-op, `Frame` replaces it with a real 77-byte, one-bool function, and
   both `CDoDSpectatorGUI` and `CBottomBar` inherit that replacement
   **unmodified** -- there never was a DoD-authored "propagate to children"
   hook to patch.

The shipped fix does not need a DoD-specific override or a live object
lookup at all, because a vtable is per-*class*: it redirects each class' own
`SetVisible` slot (in `.rdata`, fixed and RTTI-verified, the same technique
`hudelement.rs` already uses for `Draw`) to a 13-byte trampoline that forces
the boolean argument to `false` and tail-jumps into the untouched stock
function -- so every real side effect `Frame::SetVisible` has (there is an
animation controller it notifies) still runs, just always told to hide. No
`.res` file is touched, and nothing needs `mirv_recordmovie_start` running.

**Live-tested 2026-09-21, and it did not work.** The redirect installs with
no error -- RTTI checks and the stock-value check both pass -- but both bars
stayed visible. Re-checked the other candidate "overridden" slots
(27/28/29/30) hoping for a `Paint`-family fallback; none of them is one --
27 is a cached-string getter, 28 is the destructor, 29/30 are pass-through
adjustor thunks wrapping `Frame`'s own unmodified function. These container
classes apparently don't draw anything themselves, only their children
(separately-classed labels/comboboxes) do, so a render-function stub was
never going to be a full fix anyway.

That leaves one real open question, which a static scan can't answer for an
indirect (virtual) call site: is `SetVisible` ever called on these two
objects at all during ordinary play? `spectator_bars.rs` had the trampoline
increment a hit counter, surfaced in `dodstudio_debug_status`, to settle it
empirically rather than by more guessing.

**Second live test, same day: the hit count was 0.** `SetVisible` is
confirmed to never be called on either object during ordinary play, full
stop -- not a guess this time, a direct measurement. Chased the "then what
does `call 0x1957b40` do" question one step further anyway, since it no
longer mattered whether the trampoline reached it: that call disassembles
to a bare `ret 4`, an empty stub almost certainly COMDAT-folded together
with dozens of unrelated no-op stubs across the binary (including
`Panel@vgui2`'s own never-overridden base version of this exact slot) --
not a meaningful setter at all. Reading the rest of `SetVisible`'s body with
that ruled out: it never writes a persisted flag on `this` anywhere. It
reads `this`'s current visibility through `this`'s own vtable
(`[eax+0x68]`), then hands the old and new state to a *separate*
animation-controller object (`call 0x1967370` looks like
`GetAnimationController()`). `Frame::SetVisible`, in this build, does not
synchronously set anything -- it queues an animated transition on another
object. So even a working hook into this slot would only ever have queued
an animation, not forced an immediate, reliable hide.

**Next step is not more static analysis of `SetVisible`.** Two guesses in a
row (the slot, then this helper) were both wrong; a third without a new
lead would be the same kind of guessing. The reliable way forward is a live
memory watch -- a debugger attached to a running session, a hardware
write-breakpoint on the real flag, and toggling the already-working
`Spectator.res` edit to see what actually writes it.

**Sharper symptom, same day.** The user can already edit individual *items*
on the bottom bar (the mode/player/view comboboxes) via
`BottomSpectator.res` -- those respond fine to being edited. What doesn't
respond is the black background/frame itself. That changes what "the
bottom bar ignores `.res`" actually means: the children's own
`visible`/`enabled` keys clearly do reach them, so `BottomSpectator.res` is
being read and applied to *something* -- just not to whatever draws the
black backdrop. Worth checking, before any more disassembly: whether the
backdrop is `CBottomBar`'s own `Frame::PaintBackground` (inherited,
generic -- a scheme border/color resource, not a `visible` key, might gate
it) versus not being `CBottomBar` at this point in the tree at all (a
parent/sibling panel `BottomSpectator.res` doesn't even have a section for,
drawn unconditionally by something else).

**Narrower goal for next time, per a 2026-09-21 conversation with the
user:** a `dodstudio_hide_spectator_bars` command was never the requirement
-- the user already has a working, no-code fix for the top bar
(`Spectator.res`'s `visible`/`enabled` keys, edited by hand) and would be
satisfied with the same thing working for the bottom bar, instead of a
runtime command. That reframes the actual question worth investigating
next: not "how do we force `CBottomBar` hidden at runtime" but "why does
`BottomSpectator.res`'s `visible` key not reach `CBottomBar` the way
`Spectator.res`'s reaches `CDoDSpectatorGUI`". Likely worth tracing
forward from `CBottomBar`'s real construction call site (`+0x82c66`, inside
`CSpectatorGUI::CSpectatorGUI` at `+0x82b50`) to see whether it even passes
`BottomSpectator.res`'s path to a `LoadControlSettings`-equivalent, rather
than continuing to chase `SetVisible`.

---

## Reproducing this

```
python3 -c "
import importlib.util
spec = importlib.util.spec_from_file_location('survey', 'goldsrc-hooks/tools/survey_client_dll.py')
survey = importlib.util.module_from_spec(spec); spec.loader.exec_module(survey)
img = survey.Image(survey.DEFAULT_DLL)
for name, vfts in survey.vftables(img).items():
    if 'SpectatorGUI' in name or 'BottomBar' in name or name in ('.?AVPanel@vgui2@@', '.?AVFrame@vgui2@@'):
        print(name, [hex(v) for v in vfts])
"
```

§2/§3's inheritance claims come from diffing `vslots()` output for each
class against `client.dll`'s own `vgui2::Panel`/`vgui2::Frame` vtables
(`.?AVPanel@vgui2@@` / `.?AVFrame@vgui2@@`), slot for slot — not from
`vgui.dll`'s VGUI1 export table, which was the earlier (wrong) approach this
section used to take. `vgui2.dll` itself exports nothing useful (one symbol,
no `Panel`/`Frame` RTTI); the real VGUI2 control classes are statically
linked into `client.dll`.
