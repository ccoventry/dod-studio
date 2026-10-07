# The spectator top/bottom bars: why one `.res` edit sticks and the other doesn't

> **Shipped 2026-09-30 (issue #328):** `dodstudio_hide_spectator_bars 1` hides
> the spectator panel: the two dark bands and the text and menu row on them.
> It works on screen with no capture running, on both builds.
> Code: `goldsrc-hooks/src/spectator_bars.rs`. The "Shipped mechanism" section
> below is the current truth; the earlier R&D (superseded, with offsets later
> corrected) is in `docs/archive/goldsrc_spectator_bars_rnd.md`.

HLAE's `mirv_movie_hidepanels` only leaves panels out of what it *records*;
they stay on screen. `mirv_disable_specmenu` supports `tfc`/`valve` only.

---

## Shipped mechanism

**What the bands are.** Two plain `vgui2::Panel`s that `CSpectatorGUI`'s
constructor (`client.dll+0x82b50`) creates as children of the `SpectatorGUI`
frame: `topbar` (member `+0x114`) and `bottombarblank` (`+0x11c`).
`CSpectatorGUI::ApplySchemeSettings` (`+0x83030`) gives exactly these two
`SetBgColor(0, 0, 0, 196)`, and the stock `Panel::PaintBackground`
(`+0x57890`) fills them. `Spectator.res` renames the first `TopBar` (its
`fieldName`; `BuildGroup::ApplySettings` matches section names with
`_stricmp`). The frame itself paints nothing. `CBottomBar` (`bottombar`,
renamed `BottomBar`, member `+0x118`) is the transparent combobox row that
DUCK brings up. The player name on the bottom band is the label `playerlabel`.

**Why the `.res` edit hides the top band and not the bottom one.**
`"visible" "0"` reaches both panels. Nothing in the image ever calls
`SetVisible(true)` on `topbar`, so it holds. For `bottombarblank` the
constructor itself calls `SetVisible(true)` *after* `LoadControlSettings`
(`+0x82e2f..+0x82e39`), and `CSpectatorGUI::OnThink` (`+0x82ef0`) re-sizes it
and re-positions it at the bottom edge on every vgui frame, which defeats
`tall 0` and `ypos 9999` as well.

**How the cvar hides them.** Every panel's paint goes through one function,
vgui2's `IPanel::PaintTraverse` (interface `VGUI_Panel007`, vtable slot 41):
`client.dll`'s `Panel::PaintTraverse` (`+0x57560`) calls it for each child
(`+0x576f2`), and the engine calls it for the root. `spectator_bars.rs` swaps
that one vtable slot for a filter. The `SpectatorGUI` frame is not painted,
and neither is anything under it, since children are only painted from inside
their parent's paint: the bands, the score and timer text, the player label,
the DUCK row and the inset outline all go together. Nothing about the panel
is changed, so there is nothing for the game to put back, and turning the
cvar off shows it again at once.

The first version had two cvars: this one hid only the bands (`TopBar` and
`bottombarblank`, matched by name under the `SpectatorGUI` parent, since the
scoreboard has a `TopBar` of its own) and left the text floating, and
`dodstudio_hide_spectator_gui` hid the frame. The bands-only one was dropped
at the user's request and the frame behaviour took its name. Bringing it back
is one name comparison and a `GetParent` call (slot 19, `ret 4`); commit
`ffc1f434` has it.

**The interface.** `VGUI_Panel007` is the class `VPanelWrapper` in
`vgui2.dll`, 60 slots, slot 0 the virtual destructor. Used here: 36
`GetName`, 41 `PaintTraverse` (19 is `GetParent`). The two installs' `vgui2.dll`
are different files (the 25th Anniversary one was rebuilt with a newer
compiler) but the layout is identical, as it has to be: `client.dll`, compiled
against it, is byte-identical in both. `goldsrc-hooks/tools/verify_vgui2_ipanel.py`
checks the two slots against both files.

**Guards at install.** The vtable must identify itself by RTTI as
`VPanelWrapper`, the two slots must point into `vgui2.dll`'s own code, and
each must end in the `ret` its argument count demands. Otherwise nothing is
patched, and the log and console say why.

**Live result (2026-09-30, an HLTV demo in first person, both builds).**
Mean brightness of the band regions in frames recorded with
`mirv_movie_hidepanels 0`:

| build | bands showing (top / bottom) | `dodstudio_hide_spectator_bars 1` | turned off again |
| --- | --- | --- | --- |
| pre-Anniversary | 27 / 27 | 114 / 115, the scene's own level | 27 / 27 |
| 25th Anniversary | 24 / 18 | 94 / 76 | 24 / 19 |

The score, timer and player name go with the bands. Measured on the one-cvar
build; the earlier bands-only build gave the same figures. HLAE's
`mirv_movie_hidepanels` defaults to 1, so its recorded frames never show
these panels either way; this is about the screen.

**Corrections to the R&D below.**
- "Slot 8 is `SetVisible`" (§3) is wrong. Slot 8 of a vgui2 `Panel`/`Frame`
  is `OnChildAdded`; `SetVisible` is slot 29 and `IsVisible` slot 30. That is
  why the parked trampoline counted 0 hits.
- `CDoDSpectatorGUI` is at `DoDViewport+0x744`, not `+0x740`
  (`DoDViewport::VidInit` runs on the `+4` sub-object), and `CBottomBar` is at
  `CSpectatorGUI+0x118`, not `+0x114`.
- "These container classes don't draw anything themselves" was right, and the
  missing piece was that their two child `Panel`s do.

---

The earlier R&D (the two `.res` files, the RTTI survey, the `SetVisible`
dead end) is in `docs/archive/goldsrc_spectator_bars_rnd.md`.
