# `viewdemo` from the inside (issue #405)

What `viewdemo` does with a demo, established offline against both movie
installs' `DemoPlayer.dll`, `Core.dll`, `hw.dll` and `GameUI.dll`, and
cross-checked with ReHLDS's reconstruction of the same code
(`rehlds/HLTV/DemoPlayer`, `rehlds/HLTV/Core`, `rehlds/HLTV/common/DemoFile.cpp`).
Written for the question behind #405: can the capture batch move from
`playdemo` to `viewdemo`, so each clip is reached by a jump rather than a
`host_framerate` fast-forward?

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
is no route to #206.

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

`dodstudio_seek_to <seconds>` and `dodstudio_seek_by <seconds>`
(`goldsrc-hooks/src/demo_seek.rs`) jump without pausing and refuse while the
demo is still loading. With `dodstudio_seek_skip_between 1` they also move the
two "last sent" marks (`this+0x3c0`, `this+0x3d8`) to the landing frame, so
the next frame runs that frame's commands and nothing before it. That suits a
batch, whose skipped commands belong to other clips; it does not suit a POV
demo, whose type-3 frames are the player's own key presses.

## What this means for moving the batch

Nothing found blocks it. Today's patched chain demos would run under
`viewdemo` with their commands intact. The design the issue describes -- the
hook holds the clip list and seeks through it by world time -- is the one that
needs the skip: without it, a jump over earlier clips replays their
`mirv_recordmovie_start`/`stop`. Still to measure live: `dodstudio_seek_*` in
the editor, one batch under `viewdemo`, and the load times above.
