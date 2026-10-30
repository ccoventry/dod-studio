# `viewdemo` from the inside (issue #405)

What `viewdemo` does with a demo, established offline against both movie
installs' `DemoPlayer.dll`, `Core.dll`, `hw.dll` and `GameUI.dll`, and
cross-checked with ReHLDS's reconstruction of the same code
(`rehlds/HLTV/DemoPlayer`, `rehlds/HLTV/Core`, `rehlds/HLTV/common/DemoFile.cpp`).
Written for the question behind #405: can the capture batch move from
`playdemo` to `viewdemo`, so each clip is reached by a jump rather than a
`host_framerate` fast-forward?

> **Status 2026-10:** the jump is built as `ds_demo_seek_to` / `_by` /
> `ds_demo_seek_skip_between` (`goldsrc-hooks/src/demo_seek.rs`), and #405
> (batch under `viewdemo`) is still open. Long forward jumps drop joins and team
> picks (#596); a fix that steps them 5 s per frame is the open PR #601, not yet
> on `dev`.

`goldsrc-hooks/tools/verify_demo_seek_offsets.py` re-checks every binary fact
below that code depends on.

## Two different players

| | `playdemo` | `viewdemo` |
| --- | --- | --- |
| Reads the file | `hw.dll`, one frame at a time as playback reaches it | `Core.dll`'s `DemoFile`, the whole file, into an HLTV world |
| Plays back | the file stream | the world, by the player's clock (`DemoPlayer.dll`) |
| Can jump | no | yes: `IDemoPlayer::SetWorldTime` |

`DemoPlayer.dll` exports only `CreateInterface`; `demoplayer001` is a
singleton (its factory returns one static object), and `hw.dll` fetches it once
(`hw.dll+0x32698` pre-Anniversary). The vftable is the HL SDK `IDemoPlayer.h`
order, 47 slots, in both builds, although the two DLLs are different compiles
(138 KB and 48 KB). The field offsets used here are the same in both.

## 1. `ConsoleCommand` (type 3) frames: kept, and run like `playdemo` runs them

The scheduled-command mechanism depends on this, and it holds.

- **Loading.** `DemoFile::ReadDemoPacket` (`Core.dll+0x7860` pre-Anniversary)
  reads a type-3 frame as 64 bytes and appends it, with its type byte, to the
  demo-data buffer. The next network frame that carries entities gets that
  buffer as its `demoData` (`Server::ProcessMessage`), so the commands are
  stored against the world frame they were written before.
- **Playback.** `DemoPlayer::ReadDemoMessage` (slot 45) runs
  `ExecuteDemoFileCommands` on every world frame between the last one it sent
  and the one it sends now. Type 3 there calls the engine wrapper's slot 27
  (a validity check) and, if it passes, slot 28 (add to the filtered command
  buffer).
- **Same filter as `playdemo`.** In `hw.dll`, wrapper slots 27 and 28 are
  thunks to the very two functions `playdemo`'s own type-3 reader calls
  (`hw.dll+0x111b4`: `cmp al, 3`, read 64 bytes, check at `+0x1dce0`, add at
  `+0x27350`; the 25th Anniversary build has the same pair at `+0x199f25`).
  Those are also what `svc_stufftext` uses (`Server tried to send invalid
  command`, `cl_filterstuffcmd`). So any command a batch runs today under
  `playdemo` passes the same gate under `viewdemo`, including HLAE's
  `mirv_recordmovie_start`.

Caveats:

- **A forward jump runs the skipped frames' commands in one burst.** See
  "Jumping" below.
- **At most 61 per world frame.** The demo-data buffer is 4010 bytes
  (`push 0xfaa` in `Server::Init`, both builds), and each command takes 65. If
  the frames before one network frame overflow it, all of them are dropped for
  that frame, silently. The batch writes about ten at demo start (the Initial
  Commands plus the app's own), and none of the demos checked has more than 28
  in a row.
- **`dem_forcehltv 1` drops them.** `Server::ProcessMessage` keeps demo data
  only when not forcing HLTV. Default is 0. (ReHLDS source; not re-derived from
  the binary.)

## 2. The trailing directory: followed, and ours is right

`docs/goldsrc_dod_quirks.md` said `viewdemo` crashes on directory-offset
mismatches, which is why the batch used `playdemo`. There is no recorded
experiment behind it, and nothing found here reproduces it.

`DemoFile::LoadDemo` seeks to the header's directory offset, reads the entry
count and entries, and starts at `entries[0].offset`; each `NextSection`
(type 5) frame jumps to the next entry's offset. A bogus entry count falls back
to reading straight on from the header ("WARNING! Demo had bogus number of
directory entries!"); a bad frame length stops playback with a warning. It does
not read the entries' frame counts.

The patcher (`native/src/patch/engine.rs`) shifts each later entry's offset by
the bytes injected before it, grows each entry's length by what it injected,
and moves the header's directory offset by the total. A frame walk of nine
demos -- four preview demos with injected director events, two decal-flush
outputs and three others -- found every entry's offset and length equal to the
real segment boundaries. Studio's Launch Preview already opens
patched demos with `+viewdemo`.

## 3. The camera: still DoD's client

`viewdemo` does not compute the view for an HLTV demo, so it does not get
around DoD's client discarding `DRC_CMD_CHASE`/`DRC_CMD_INEYE` (#222), and it
is no route to #206 (closed since: `ds_spec_lock`, see
`docs/goldsrc_spectator_camera.md`).

- `Server::ParseHLTV` marks an HLTV demo (`HLTV_ACTIVE`), and an HLTV world's
  frames get no `demoInfo` (the recording client's view).
  `IDemoPlayer::GetDemoViewInfo` returns without touching the view when a frame
  has none (`DemoPlayer.dll+0x3b92`: `frame->demoInfo` at `+0x54`). A POV demo
  does carry it, and there `viewdemo` does drive the view from the recording.
- Director events are not dropped either. `Server::ParseDirector` hands each
  `svc_director` to the player's events list (the editor's list) instead of the
  frame, and `WriteCommands` sends them to the client as `svc_director` when
  playback crosses their time. `DRC_CMD_CAMERA` is sent with its entity
  zeroed, `DRC_CMD_TIMESCALE` is applied by the player itself, and everything
  else, `INEYE` and `CHASE` included, reaches DoD's client exactly as under
  `playdemo`.

## 4. HLAE recording

Settled by use: the user has captured movie clips under `viewdemo` for years,
always after the demo finished loading. Item 1 adds that the batch's own
injected commands reach the console the same way they do now.

## 5. Load time

The whole demo is read into the world before `IsLoading()` clears, at most 33
network packets per engine frame (`Server::RunFrame`, `cmp eax, 0x20; jg` at
`Core.dll+0x10db3`), while playback has already started. Network frames in
three demos, and the engine frames that takes:

| demo | size | length | network frames | engine frames | at 100 fps | at 300 fps |
| --- | --- | --- | --- | --- | --- | --- |
| `wsod25_grp2_h2_hltv_8_preview` | 29 MB | 21 min | 40,805 | 1,237 | 12 s | 4 s |
| `bandits-ktps5w10-map2-anzio-over-allies-milo` | 91 MB | 21 min | 113,764 | 3,447 | 34 s | 11 s |
| `wsod25_ply2_m3_h2_dyelife` | 450 MB | 125 min | 672,418 | 20,376 | 204 s | 68 s |

What frame rate the engine holds while it loads is not measured, and neither is
memory: the world keeps every frame (`SetBufferSize(-1)`), inside a 32-bit
process. The console prints `Demo file completely loaded.` when the loader
disconnects, which is what `IsLoading()` reports.

Measured on #797 (both builds, the DoD Studio window open or closed): the
loader runs about 163 engine frames a second, 59.6 s of demo per wall second.

**With the hook DLL** the cap is a time budget instead (#798,
`ds_demo_fast_load`, on by default, `goldsrc-hooks/src/fast_load.rs`).
A detour over the counter's `inc eax` through its `cmp eax, 0x20`
(pre-Anniversary `Core.dll+0x10db2`, 8 bytes; 25th Anniversary `+0x12db9`,
7 bytes) redoes the increment and the store, calls into the hook, and
compares the count against what it returns, so the engine's own `jg` still
decides: no cap while this frame's loading has taken under the budget (8 ms
by default), the engine's `0x20` once it has. A frame therefore still reads
at least 33 packets. `0` returns `0x20` every time, the engine's own decision.
`Core.dll` loads with the first `viewdemo`, so the patch goes in then, found
by signature; `tools/verify_fast_load_offsets.py` checks both builds' loops
(the count's init, the span, what follows the `jg`). The hook log has one
`fast_load:` line per load with its time, packets and frame times.

`Core.dll` also holds 256 entities per frame on the pre-Anniversary build
(1024 on the Anniversary one); see the entity-limit entry in
`docs/goldsrc_dod_quirks.md`. That limit is the same `viewdemo` or not.

Why ESC reloads a demo under `-demoedit` was not found offline.

## Jumping

The events list's **Goto** (`GameUI.dll`, `CDemoPlayerDialog`) is
`SetWorldTime(event time, false)`, `ExecuteDirectorCmd(event)`,
`SetPaused(true)`. `SetWorldTime` only stores the clock; the next frame does
the rest, and a forward jump is **not** a teleport:

- `WriteCommands(last frame time, clock)` sends every director event between
  the two, and only the clock moved.
- The type-3 loop runs every world frame's commands from the last one sent to
  the new one.

A backward jump runs neither, since both ranges are empty. So "events between
the old and new position do not fire" is true going back and false going
forward, where the in-between events arrive all at once and only the last
camera state is visible.

The engine already has `dem_jump <seconds>` (relative, then pauses),
`dem_start`, `dem_pause <0|1>`, `dem_speed <rate>` and `dem_save`, all from
`DemoPlayer.dll`.

`ds_demo_seek_to <seconds>` and `ds_demo_seek_by <seconds>`
(`goldsrc-hooks/src/demo_seek.rs`) jump without pausing and refuse while the
demo is still loading. With `ds_demo_seek_skip_between 1` they also move the
two "last sent" marks (`this+0x3c0`, `this+0x3d8`) to the landing frame, so
the next frame runs that frame's commands and nothing before it. That suits a
batch, whose skipped commands belong to other clips; it does not suit a POV
demo, whose type-3 frames are the player's own key presses.

### One message per jump, and what overflows it (#596)

The player sends a jump's whole catch-up as **one** message, built in its
64 KB `m_DemoStream` (ReHLDS `DemoPlayer::WriteDatagram`):

- `World::WriteFrame(frame, lastFrameSeqnr, ...)` adds the reliable data and
  user messages of every frame after the last one sent, each only if it still
  fits. That is where a jumped-over player's `svc_updateuserinfo`, team and
  score messages travel.
- `WriteCommands` then adds the director events with no such check. A stream
  that overflows is cleared whole, the console prints `Demo data stream
  overflow.`, and the player carries on from the landing frame alone.

A 20-minute jump in an HLTV demo did exactly that on 2026-10-04
(`ds_demo_seek_to: 90.10 -> 1335.30 s`, then the overflow line): a player
who joined 6:42 into the recording had no name in the kill feed and no team,
the scoreboard counted 0 Allies and 0 Axis, and `ds_spec_target` found
no such player. Players already on the server when the recording began were
fine, since their info is in the signon data.

So `ds_demo_seek_to`/`_by` take a forward jump of more than 5 seconds in
5-second steps, one per sent frame (each step waits for `m_LastFrameTime` to
reach it). Measured with `analysis/examples/seek_burst.rs` on four HLTV
demos: no 5-second window held more than 55 KB of network data, entities
included, and the catch-up leaves entities out. `ds_demo_seek_skip_between 1`
jumps at once, as before.

### Backward jumps in an HLTV demo (#830)

A backward jump replays nothing: `WriteDatagram` moves the "last sent" mark
to the frame before the landing, so only the landing frame goes out, and the
client keeps whatever the later position left it. In a POV demo that is
mostly harmless (the zoom, for one, is in every frame's client payload). In
an HLTV demo each player's zoom comes only from the `HLTV` user message,
sent once per change and kept by `client.dll` in a 64-slot table nothing
else resets (`goldsrc-hooks/src/hltv_fov.rs`): after Review's Replay a sniper
scoped in at the landing showed unzoomed, because he had unscoped by the old
position. Teams, scores and names had the same staleness.

So in an HLTV demo `ds_demo_seek_to`/`_by` take a backward jump as: clear the
zoom table, set the clock to the demo's start, wait for that frame to be
sent, then step forward to the target as above. Every message before the
landing arrives in order. On `wsod25_grp2_h1_hltv` a jump back to 15:40
took 182 steps, 1.4 s on PRE and 1.7 s on POST. The table is also cleared
whenever a demo is loaded, since `client.dll` stays loaded between demos.
POV demos and `ds_demo_seek_skip_between 1` still jump back at once.

### The replayed console commands (#702)

A POV demo's type-3 frames (`+forward`, `-moveleft`, `+speed`...) are
replayed by the steps too, into `hw.dll`'s `filteredcmd_text`, the 16 KB
buffer for text the game didn't type itself. `Cbuf_Execute` runs that buffer
**one command per frame** (both builds), so a 30-minute jump's ~15,000
commands overflowed it about 6,600 times and then drained for 40 seconds of
the demo after the landing, one stale key press a frame. DoD's gun lowering
reads those keys, so the crosshair blinked through the first clip of a
capture batch. A stepped seek now empties that buffer every frame until it
lands, then sends `-forward`, `-speed` and the rest
(`goldsrc-hooks/src/filtered_commands.rs`). The 31:01 clip of
wsod25_grp2_h1_m00cat (PRE): 18 blinks under 0.1 s before, none after, the
same as reaching it by fast-forward.

## Demos on another drive (#376, #776)

`viewdemo` and `playdemo` take a path relative to `dod\`, `../` included,
and nothing else. Where that comes from, read out of both installs' DLLs
(#776): every demo open ends in the file system's `Open(name, "rb", pathID
= 0)` -- `hw.dll`'s `FS_Open` for `playdemo`, `Core.dll`'s
`DemoFile::LoadDemo` (through `DemoPlayer.dll`'s `IDemoPlayer::LoadDemo`,
slot 41) for `viewdemo` -- and `FileSystem_Stdio.dll`'s `Open` only tries
`<search path> + name` for each search path in turn. Nothing on the way
checks the name (the "Relative pathnames are not allowed" test belongs to
`record`, `appenddemo` and the other demo-editing commands). So on the
pre-Anniversary build `viewdemo "D:/x"` is "Coudn't open demo file"; on the
25th Anniversary build its `Open` first asserts `!strchr(pFileName, ':')`
(`BaseFileSystem.cpp` line 471, `_wassert`: a fatal dialog), the assert the
old version of this section mentioned without a place.

**With the hook DLL**, `viewdemo "D:/folder/demo"` works
(`ds_demo_other_drives`, `goldsrc-hooks/src/demo_other_drives.rs`):
its `Open` slot adds `D:\` as a search path the first time the drive is
named (`AddSearchPath` takes a path with a drive letter as it is and
appends it last; its own path ID keeps the engine's `GAME` lookups out of
it) and opens `folder/demo.dem` under it, before the original `Open` and
its assert. `tools/verify_demo_other_drives.py` checks the slots and the
assert on both installs. Studio uses it (#779): a Seek Between Clips
batch, Review highlights and Launch Preview play a demo on another drive
by its full path (the hook's `review::viewdemo_line`, Studio's
`native::preview_in_place`), with `ds_demo_other_drives 1` sent first.
Studio knows a DLL can by the marker `demo_other_drives::FULL_PATH_MARKER`
in the DLL file; for an older DLL a batch copies such a demo into `dod\`
(`native::patch::source_copies`) and a preview is patched.

**Without it**, a demo on `D:\` needs a path under `dod\`. A directory
junction gives it one with no copy and no engine change: Studio's + Link
Folder to Game makes `dod\demos\<name>` point at the folder
(`native::demo_links`), and the game follows it like any folder, for
opening and for the Load Demo list (#409).

The link's rules, each there so no folder walk can loop through it: it
points at the folder picked, never a drive root; a folder that holds the
install or is inside it is refused, as are links and network paths; Remove
unlinks the junction only (`RemoveDirectoryW` on the link, never a
recursive delete). Studio's own walks skip every junction and symlink and
stop at a depth cap (`native::sys::folder_walk`); the hook's walks read the
entry kind from the listing, which never follows a link.

Checked in game on the Anniversary install (#764): `viewdemo
"demos/<link>/<demo>"` from the console, and the link's folder and demos in
the Load Demo window. Still unchecked: the same path on the launch line
(`+viewdemo "demos/<link>/<demo>"`).

## What this means for moving the batch

Nothing found blocks it. Today's patched chain demos would run under
`viewdemo` with their commands intact. The design the issue describes -- the
hook holds the clip list and seeks through it by world time -- is the one that
needs the skip: without it, a jump over earlier clips replays their
`mirv_recordmovie_start`/`stop`. Still to measure live: one batch under
`viewdemo`, and the load times above. (`ds_demo_seek_*` is in use; its
forward-jump problem is #596.)

### The first ESC after a second demo (#663)

GameUI's VCR bar (`CDemoPlayerDialog`) treats "the slider's value differs
from the one I set last" as the user dragging it, and seeks there and
pauses. Its `OnThink` only runs while the bar shows, so after a second
`viewdemo` the hidden bar still holds the first demo's slider state, and the
first ESC sent playback to the new demo's first whole second, paused (PRE,
traced: `SetWorldTime(726.00)`, `SetPaused(1)` from 1706 s). The hook keeps
the hidden bar's slider range, value and last value in step with playback
(`goldsrc-hooks/src/studio_panel/hook/vcr_sync.rs`), so that first think
finds nothing changed.
