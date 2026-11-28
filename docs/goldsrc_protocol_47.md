# Protocol-47 demos

A GoldSrc demo names the network protocol it was recorded with twice: in its file header and in every `svc_serverinfo` message (one per map). Today's game, pre-Anniversary and 25th Anniversary alike, is protocol 48, and so is every demo in the PRE and POST libraries. Protocol 47 is the one before it: DoD 1.3 demos from the early Steam years, 2004 and a little after. This page says what the game does with one, what differs between the two, how DoD Studio reads and converts a protocol-47 demo, and how that was established (#239).

Measured on a community archive of 2004-2008 DoD 1.3 match demos (not in the repo: the files are other people's recordings): 103 files, 78 of protocol 47, 23 of protocol 48, and two damaged copies with no demo header. The protocol-47 set covers seven maps (dod_donner, dod_chemille, dod_anzio, dod_harrington, dod_railroad, dod_railroad2_b2, dod_lennon_b2), 71 HLTV recordings and 7 POV ones, several matches per map and many recorders.

## What the game does with one

It depends on how the demo is started. GoldSrc plays a demo two ways, and they check the protocol in different places (#1549).

- **`playdemo`** (hw.dll plays the file itself) warns, then reads it as it reads a protocol-48 demo:

      WARNING! demo protocol outdated.
      Demo file protocols 47:5, Engine protocol is at 48:5

  hw.dll's `CL_ParseServerInfo` does compare the server info's protocol with 48 (`Server returned version %i, not %i`), but it skips the check while a demo plays (`cls.demoplayback`; PRE hw rva 0x1c63b-0x1c657, POST 0x1a7975-0x1a7990). So that line never appears, converted or not.
- **`viewdemo`** (the demo player, `demoplayer.dll` over the HLTV proxy's `Core.dll`) **refuses it**. Core.dll's header check only warns (`WARNING! <demo> has an outdated demo format.`). Its server-info parser, though, disconnects on any protocol but the one its server object holds, which is 48. It is set at start-up (PRE rva 0x1082d, POST 0xf7f9), and only the standalone HLTV proxy ever changes it (`Server::SetProtocol`, vftable +0x94, which accepts 46-48; nothing in Core.dll or demoplayer.dll calls it, and the game never loads proxy.dll):

      WARNING! Server returned protocol version 47, not 48
      Disconnected.
      Demo playback stopped.

  Nothing loads. Measured on PRE and POST with an archive HLTV demo (harrington-x6-pandemic-1) and two February 2004 HLTV demos (shock vs 3h, dod_merderet). The same files play under `playdemo`, and their converted copies play under `viewdemo`. Offsets: Core.dll `ParseServerinfo` check PRE rva 0x11b18-0x11b3d, POST 0x1104d-0x11074; `SetProtocol` PRE 0x13850, POST 0x13510. The check reads only the server info's protocol, so the demo protocol, HLTV or POV, and the spawn count make no difference (POV demos take the same parser, by the code; not run).

Studio's previews, Seek Between Clips batches, Review, and the in-game Load Demo window and Demos tab all use `viewdemo`, so each plays a converted copy of a protocol-47 demo (see "Converting one"). Patched batches convert as they copy and start with `playdemo`.

- **`playdemo` under Studio's launch flags** (HLAE with the hook DLL), on the PRE and POST movie installs: an HLTV and a POV demo played to the end unconverted, as their converted copies did, and a demo with two server infos (a restart) reloaded at the restart and played on.
- **Without the hook, some of them crash, for a reason that isn't the protocol.** Two demos tested, each unconverted and converted, on both builds, with the game launched four ways (plain `hl.exe` with Studio's flags; HLAE's `AfxHookGoldSrc.dll` only; the hook DLL only; Studio's full launch):
  - harrington-x6-pandemic-1 (HLTV) crashes a few seconds in whenever the hook DLL is absent, converted or not: POST with an access violation at `hw.dll+0x1c793b` (`EV_SetTraceHull` writing through a null `pmove`), PRE at `hw.dll+0x3a77c`, which without the hook's crash recorder shows only as a silent exit with code 0. The demo sends DoD's InitHUD in its first packets, before the engine has set `pmove`: the start-up crash the hook's `pmove_guard` stops (#546). With the hook DLL alone it plays on both builds; with `GOLDSRC_HOOKS_PMOVE_GUARD=0` the crash comes back. HLAE's own DLL and the `-insecure` / `-demoedit` flags make no difference.
  - tpg-lennon-mania-axis (POV) plays on plain `hl.exe` on both builds, unconverted.
  - So the crash can hit any first demo of a session that sends InitHUD early, protocol 47 or 48, in a game without the hook. Converting changed nothing in any of the 16 runs. The console's `Protocol Version 47, Spawn count N (HLTV)` line stays on a converted HLTV copy: it is text the HLTV proxy recorded, not the engine's check.

Two early-2004 demos of the archive (GameWyze-mts-donner-1 and -2) crash the game at load, converted or not, in `client.dll` (a null model header read while looping over a model's attachments). That is not a protocol matter.

## What differs between 47 and 48

Only the protocol number, in the header and in each server info. Measured with `analysis/examples/protocol47_probe.rs` over all 101 readable demos:

| | Protocol 47 (78 demos) | Protocol 48 (23 demos) |
| --- | --- | --- |
| Header network protocol | 47 | 48 |
| Header demo protocol | 5 | 5 |
| `svc_serverinfo` protocol | 47 in every one (82 in all: four demos load their map a second time, a restart, and carry a second server info for the same map) | 48 |
| `svc_version` | none in any demo | none |
| Engine messages that occur | the same ids; `svc_resourcelocation` (56) in some of each | the same |
| `svc_serverinfo` VAC byte, and the 21-byte old-build block (#1456) | 0, none | 0, none |
| Delta descriptions | sent in the demo; the same tables. 18 early-2004 recordings carry `clientdata_t` with 43 fields and `weapon_data_t` with 15, the rest 39 and 10 | 39 and 10 |

The layout of everything else is protocol 48's. Every frame and message of every protocol-47 demo parses with the protocol-48 parsers in `dem-patch` (which refuse to leave a byte of a message unread), and the analyzer reads each to the same map, players, kills and rounds it would read from a protocol-48 demo. The delta tables need nothing either way: a demo carries its own (`svc_deltadescription`), and the decoder reads whatever it is given.

The server build doesn't tell the two apart. In this archive the POV demos from servers of build 3382, 3647 and 3651 are protocol 47 and those from builds 4554, 5956 and 8196 are 48, but #1456's two demos from a build-2693 server say 48 in both places.

### Not a protocol difference: two next-section frames

Two of the protocol-47 demos (three files: one is a copy under two names) end their LOADING segment with two next-section frames instead of one. `dem-patch` ends the segment at the first, so a demo it writes back drops the second; the directory still points at the right places. HLAE's DemoTools handles the same thing ("munch repeated stops"). The conversion never re-encodes, so it keeps them.

### Demos HLAE's demo tools rewrote

Old demos run through HLAE's demo tools (DemoFix with its protocol 47 to 48 option, advancedfx `demotools.cpp`) come out protocol 48 with the frames unchanged but the header and directory rewritten (#1541; six 2004 HLTV demos from three matches):
- The 260-byte map-name and game-folder fields hold HLAE's watermark text after the NUL. Read them up to the first NUL, as the engine does.
- Every directory `track_time` is 0 and `cd_track` is 255. The length comes from each playback entry's closing next-section frame instead, whose time is what the engine writes as the track time (the same to the millisecond on 492 demos that have both).
- A third, 9-byte directory entry of type 2 follows the playback one: a stray demo-start frame and a next-section, read as garbage from the original directory. `dem-patch` writes it back as 18 bytes, so such a demo does not round-trip byte for byte.

## Reading one

`dem-patch` reads protocols 47 and 48 and refuses any other by number in the header parse (`dem-patch/src/protocol.rs`): "network protocol N is not supported". No other protocol has been checked against a real demo, so one is not read with rules that may not hold for it. The Demo Analyzer's Technical Specifications show a protocol-47 demo's network protocol as "47 (older protocol): DoD Studio converts a copy when it plays it".

## Converting one

Converting is what lets `viewdemo` play one (Studio's previews, Seek Between Clips batches, Review, the in-game Load Demo window); under `playdemo` it only removes the warning. `native::patch::protocol47` rewrites the protocol in place: the header's protocol, and the protocol of each `svc_serverinfo` saying 47. It searches every byte from the header to the directory (or to the end of a file whose directory is missing) for a server info: its id and protocol, confirmed by a game folder, a host name and a `maps/*.bsp` where they belong. Searching the bytes rather than only the frames a walk reaches matters for a damaged demo: a broken frame length stops a walk, or carries it past a restart's server info, and a server info left at 47 would leave the copy half converted. Over the 78 demos the search finds exactly the server infos a frame walk does. A walk of the frames by their own lengths still runs, to say where a damaged copy's frames stop fitting: the CLI reports it, since the game may not play past that point. Every other byte stays the recorder's; the length doesn't change, so the directory needs nothing. A demo with no server info to rewrite is not converted: its copy would say 48 in the header and 47 where it loads the map, a mix no recording has.

Where a protocol-47 demo is converted:

- `dod-studio-cli convert-47 <demo>...` writes `<name>_p48.dem` beside each demo (and refuses one with no server info, writing nothing), or `--output <file>` for one. The demo itself is only read, and the copy never goes over it: it is written to a new temporary file beside its final name (one that did not exist before, so neither the demo nor anyone else's `.part` file is ever opened for writing) and renamed into place. A conversion reads and writes the file once: about half a second at most over the 78 demos above (9 to 91 MB, local disk, the CLI's start included). One of the 78 has frames that stop fitting before its end (a damaged copy: a frame zeroed 8.5 MB in, and cut short), which the CLI reports.
- A patched copy says 48 by itself: `StreamPatcher` (Studio's patched batches, `preview_cli`) and `dod-studio-cli patch-streak` convert as they copy, so a batch from a protocol-47 demo plays without the warning (checked on PRE and POST). The patcher logs a line in the activity log when it converts one. One it finds no server info in is still patched, and its copy stays protocol 47 throughout, header included, since the game plays that with its warning; the patcher logs that instead. `patch-streak` prints the same two cases.
- Studio's `viewdemo` paths play a copy in its own folder, `%LOCALAPPDATA%\dod-studio\protocol48_demos\v<N>-<key>\<stem>_p48.dem` (`native::patch::protocol48_copies`, #1549), never the original and never a file in the game's folders. The key is the demo's size-and-header key, so a copy is made once per demo (about half a second, logged in the activity log with its time) and reused by every later play, and a different file under the same name gets its own. The newest 8 stay; a running batch's are never removed. Each path:
  - an in-place Launch Preview converts the demo (or its trimmed copy, #207) and plays the copy, with its steps on the Launch Preview button;
  - a Seek Between Clips batch converts each protocol-47 demo it plays before it starts, on the Capture footer ("converting"), and the batch file names the copies;
  - a review sends the game the copies in its queue;
  - the in-game Load Demo window, the Demos tab and a typed `viewdemo`: the hook DLL reads the first 16 bytes of the file the name opens, and for a protocol-47 demo doesn't run `viewdemo` (it would only disconnect). It asks Studio over its game pipe for the copy (`convert <path>`, answered `copy <path>` or `failed <why>` by `native::sys::game_convert_requests`) and plays it once Studio answers (`goldsrc-hooks/src/old_protocol.rs`). With Studio closed, the console says to open it and load the demo again, or to use `playdemo`, and the screen says to look at the console.
- Anything else that plays the original file with `viewdemo` (a game without the hook DLL) is refused: Core.dll disconnects at the server info. `playdemo` plays it with the warning.

The idea and the two places to change come from HLAE's DemoTools wizard, "Try to convert demo version (47 -> 48)": `AfxCppCli/old/tools/demotools/demotools.cpp` (`bSet_NetworkVersion` for the header, `bSet_ProtocolVersion` in `copy_gamedata` for the server info) and `demotoolswizard.cpp`, advancedfx commit `41b158171b275f9bb8ead737e8c7d0a159daf0af`, MIT (`third_party_notices.md`). Their tool reads each network frame's first messages one by one (`svc_nop`, `svc_time`, `svc_print`, `svc_hltv`) and stops at the first other one before the server info; ours finds the server info wherever it sits, past a damaged frame too. The measurements above are our own.

## Checking it

- Unit tests in `native/src/patch/protocol47.rs` on synthetic demos and on `test-fixtures/ci_fixture.dem` made to say 47, which converts back to the fixture byte for byte.
- `real_protocol_47_demos_convert_parse_and_analyse_the_same`, `#[ignore]`d, over a folder of real protocol-47 demos:

      DOD_PROTOCOL47_DEMOS=<folder> cargo test -p native --release --lib -- --ignored --nocapture real_protocol_47

  For each demo: only the header byte and one byte per server info change (47 to 48), the copy parses as strictly as the original, and the analyzer reads the same map, players, kills and rounds from both.
- `cargo run -p analysis --release --example protocol47_probe -- <folder>` prints the table's columns per demo.
