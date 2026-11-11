# `viewdemo` and a demo's map changes (issue #1372)

Why `viewdemo` stops at the end of a multi-map demo's first map while
`playdemo` plays through every map change, on both engine builds, and an R&D
hook setting that makes `viewdemo` carry on: `ds_debug_demo_play_next_map`
(`goldsrc-hooks/src/demo_next_map.rs`).

Read `goldsrc_viewdemo.md` first for how `viewdemo` plays a demo at all.
Offsets are `module+<rva>` for the pre-Anniversary and 25th Anniversary
movie installs (`Core.dll` 281,496 and 191,840 bytes, `DemoPlayer.dll`
138,152 and 47,968 bytes); `goldsrc-hooks/tools/verify_demo_next_map_offsets.py`
re-checks every one the hook depends on, against both. The engine's own
source isn't available; names come from ReHLDS's reconstruction of the same
code (`rehlds/HLTV/Core/src/Server.cpp`, `World.cpp`, `DemoPlayer/src/DemoPlayer.cpp`,
`common/DemoFile.cpp`), and each claim below was checked against the
disassembly of both builds.

## The cause: the loader ends at a level change, on purpose

`viewdemo` has two halves that run at different speeds:

- **The loader** (`Core.dll`: `DemoFile` reads the file, `Server` parses each
  message, `World` keeps the frames) reads the demo as fast as the frame
  allows, starting the moment the demo is opened.
- **The player** (`DemoPlayer.dll`) plays the world back by its own clock and
  hands the engine one message per frame.

So the loader is minutes ahead of playback, and a world holds one map (one
set of baselines, one resource list, one clock). Reading on into the next
map would replace the map being watched. Valve's loader ends instead, in two
places in `Server`, the same on both builds:

```text
Server::ParseStuffText, on the stufftext "reconnect":
    if (IsDemoFile()) Disconnect();
    else if (m_DelayReconnect) ...        // an HLTV proxy's own cases
    else Reconnect();

Server::ParseServerinfo:
    if (IsDemoFile() && m_ServerState == SERVER_RUNNING) { Disconnect(); return; }
```

`Disconnect()` on a demo closes the file (`DemoFile::StopPlayBack`), prints
`Demo playback stopped.`, marks the world complete and fires the signal
`DemoPlayer::ReceiveSignal` prints `Demo file completely loaded.` for.
Playback then runs to the world's last frame, where `DemoPlayer::RunClocks`
clamps the clock: the picture stays on map 1's last frame, and the demo bar's
end is map 1's end.

| | pre-Anniversary | 25th Anniversary |
| --- | --- | --- |
| `Server::ParseStuffText` | `Core.dll+0x12230` | `Core.dll+0x11890` |
| its `reconnect` branch: `IsDemoFile()` then `call [eax+0x48]` (`Disconnect`) | call at `+0x12410` | call at `+0x11a68` |
| `Server::ParseServerinfo` | `+0x11ad0` | `+0x10ff0` |
| its `IsDemoFile() && m_ServerState == 7` test | `+0x11ada` .. `+0x11af0` | `+0x11006` .. `+0x1101e` |
| `Server` vftable (RTTI) | `+0x31aa4` | `+0x24ab0` |
| `Server::Reconnect` (slot 17, `+0x44`) | `+0x13f20` | `+0x12a40` |
| `Server::Disconnect` (slot 18, `+0x48`) | `+0x13f80` | `+0xf2d0` |
| `Server::IsConnected` (slot 23): `m_ServerState >= 6` | `+0x141a0` | `+0xfac0` |
| `Server::IsDemoFile` (slot 24): `add ecx, m_DemoFile; jmp DemoFile::IsPlaying` | `+0x141b0` | `+0xfad0` |
| `Server::m_ServerState` (2 disconnected, 6 connected, 7 running) | `+0x1794` | `+0x1794` |
| `Server::m_DemoFile` | `+0x16de0` | `+0x569e0` |
| `DemoFile::ReadDemoPacket` | `+0x7860` | `+0x4be0` |
| `DemoFile::m_FileHandle`, `m_nextReadTime`, `m_Continuous` | `+0x6c8`, `+0xb5c`, `+0xb74` | the same |
| `DemoPlayer::ReceiveSignal` | `DemoPlayer.dll+0x43e0` | `DemoPlayer.dll+0x39a0` |
| `DemoPlayer::NewGame` (slot 15) | `+0x4500` | `+0x3510` |
| `IDemoPlayer::IsLoading` (slot 28): `m_Server->IsConnected()` | `+0x3030` | `+0x3430` |
| `DemoPlayer::ReadDemoMessage` (slot 45) | `+0x4090` | `+0x35a0` |
| `DemoPlayer::m_Server`, `m_PlayerState` | `+0x150`, `+0x37c` | the same |

### It is not the hook DLL's `reconnect` wrappers

The demo's `reconnect` never reaches the engine under `viewdemo`:
`ParseStuffText` returns before adding it to the world's data, so `hw.dll`'s
`reconnect` command (pre-Anniversary `hw+0x59e20`, Anniversary `hw+0x1cec00`),
and `connect_guard` and `review_lock` around it, don't run at all.

### Why `playdemo` gets through

`playdemo` has no world. `hw.dll` streams the file one frame at a time as
playback reaches it (`goldsrc_hw_dll_survey.md` §13, §14), so the level
change arrives when it is due: the `reconnect` stufftext runs
`Host_Reconnect_f` (`cls.state = ca_connected`, `cls.signon = 0`, the loading
plaque), and the `svc_serverinfo` that follows runs `CL_ParseServerInfo`
(pre-Anniversary `hw+0x1c5a0`, Anniversary `hw+0x1a78c0`), which clears the
client's state and loads the next map from the signon the demo carries inline
(`goldsrc_demo_salvage.md`).

## Every map change has the `reconnect`

`analysis/examples/map_change_probe.rs` prints, for every `svc_serverinfo`
after a demo's first, whether a `reconnect` came before it. Over every
multi-map demo Studio's Split Maps scan knows on this PC (2026-11-10: 145
files, unique by name and size, from many recorders; one did not parse):
**144 demos, 181 map changes, and all 181 have the `reconnect`**, always the
last message of its packet, 10 to 170 network frames before the
`svc_serverinfo` (median 15). 113 demos signon twice, 28 three times, 2 four
times and one seven times. `ParseServerinfo`'s own stop, a serverinfo with no
`reconnect` before it, was not met once.

## Why it can carry on

Everything past the stop already handles a new map on the same objects: an
HLTV proxy needs it when its server changes level, and `DemoPlayer.dll` is
built on the same `Server` and `World`.

- `ParseServerinfo` calls `World::NewGame`, which empties the world and is
  allowed from a running or complete one. The new map's first frame calls
  `World::ConnectionComplete`, which fires the world's signal 2.
- `DemoPlayer::ReceiveSignal` answers that signal with `DemoPlayer::NewGame`:
  the player's state goes back to CONNECTING, and its next two
  `ReadDemoMessage` calls send the engine a fresh `svc_serverinfo` with the
  resource list (`World::WriteNewData`) and then the spawn data, exactly as
  for the first map.
- The engine takes a second `svc_serverinfo` mid-playback as it does under
  `playdemo`: `CL_ParseServerInfo` clears the client state (`CL_ClearState`,
  pre-Anniversary `hw+0x177b0`, zeroes `cls.signon` through
  `Host_ClearMemory`), so the `svc_signonnum` that follows isn't refused as
  "Received signon %i when at %i" (pre-Anniversary `hw+0x1e370`).

Only the timing is missing: the loader must not read the next map until
playback has reached the end of this one. `DemoFile::ReadDemoPacket` already
has a throttle for that, meant for a demo fed to a proxy in real time: it
returns at once while `m_nextReadTime` is ahead of the clock and
`m_Continuous` is set. `viewdemo` loads with `m_Continuous` off. And the
loader's channel is a fake one (no socket), which `NetChannel::IsTimedOut`
never times out, so it can wait as long as a map lasts.

## The prototype: `ds_debug_demo_play_next_map`

`goldsrc-hooks/src/demo_next_map.rs`, off by default; set it before the demo
is loaded, since the loader reaches the map change seconds after `viewdemo`.

1. **The `reconnect` calls `Server::Reconnect` instead of
   `Server::Disconnect`.** One byte: the vftable offset in the branch's call,
   `0x48` to `0x44`. `Reconnect` is what a live proxy runs there (back to the
   connected state, the channel cleared), so the signon that follows passes
   `ParseServerinfo`'s test. `0` writes `0x48` back.
2. **`Server::Reconnect` (vftable slot 17) is wrapped** so the hook hears the
   map change, and holds the loader: `m_nextReadTime` to the largest float and
   `m_Continuous` on. No code is patched for the hold. In the game only the
   patched branch reaches that slot.
3. **`IDemoPlayer::IsLoading` (slot 28) says no while the loader is held**:
   the map being played is all there. The demo bar, the loading strip and
   `ds_demo_seek_to` (which refuses while a demo loads) treat it as loaded.
4. **Each frame, the hook lets the loader go** once the player is running,
   not paused, its clock is on the world's last frame and that frame is the
   last one sent (`demo_seek::played_to_end`): `m_Continuous` off again. The
   loader reads the next signon, the world starts over, and the player
   reconnects the engine to the next map.

Nothing is touched until the setting is first turned on. The value `2` also
runs the engine's own `reconnect` when it lets go, as `playdemo` does at that
point, to compare.

## Measured

All with `goldsrc-hooks/tools/game_probe.py`, the hook DLL from this branch,
the game kept behind other windows, on both movie installs, 2026-11-10 and
11-11. Demos (pre-Anniversary install's `dod` folder, played on the
Anniversary install by a `../../` path):

- `bewton-playoffs-round1-armory-allied` (POV): `dod_armory_b6` to 1210.00 s
  of the player's clock (3,910 network frames), `dod_anzio` from 4.58 to
  29.79 s (2,496 frames), then `dod_armory_b6` again from 3.18 to 1316.60 s.
- `ktps8w10qf-m00cat_soul_m2_harr_h2` (POV): `dod_harrington` to 1430 s
  (123,865 frames), then 8 s of `dod_anzio`.

The player's clock is each map's own `svc_time`, so it starts again at every
map change.

### Stock: the loader stops while playback is still at the start

`ds_debug_demo_play_next_map 0`, and the connect guard switched off
(`GOLDSRC_HOOKS_ALLOW_CONNECT=1`) so no wrapper of ours sits on `reconnect`:

| | pre-Anniversary | 25th Anniversary |
| --- | --- | --- |
| console, as the loader reaches the change | `Demo file completely loaded.` twice, `Demo playback stopped.` | `Demo file completely loaded.`, `Disconnected.`, `Demo playback stopped.` |
| loaded | 3,899 packets | 3,899 packets |
| playback then | 1178 s up to 1210.00 s, and stays there (6 readings 10 s apart) | 1173 s up to 1210.00 s, and stays there |

The stop comes seconds after `viewdemo`, long before playback is anywhere
near the map change, and with nothing of the hook's on `reconnect`: it is the
loader's own.

### With `ds_debug_demo_play_next_map 1`

| | pre-Anniversary | 25th Anniversary |
| --- | --- | --- |
| at load | the loader waits at 1210.00 s; `IsLoading` no, the loading strip goes | the same |
| playback reaches 1210.00 s | `dod_anzio` loaded 2.5 s later, clock 4.58-29.79 | `dod_anzio` loaded 6.9 s later, the same clock |
| playback reaches 29.79 s | `dod_armory_b6` loaded 2.3 s later, clock from 3.18 | loaded 4.4 s later, the same clock |
| the last map | loads whole (128,201 packets, to 1316.60 s) and ends as any demo does: `Demo file completely loaded.`, `Disconnected.`, `Automatic connection retry...` | the same |

The console prints the engine's own `Protocol Version 48, Spawn count 53`
(then `54`) at each change. Frames recorded with HLAE after both changes show
the world, the gun and the HUD drawn as usual on both builds.

