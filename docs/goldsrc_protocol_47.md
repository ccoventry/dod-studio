# Protocol-47 demos

A GoldSrc demo names the network protocol it was recorded with twice: in its file header and in every `svc_serverinfo` message (one per map). Today's game, pre-Anniversary and 25th Anniversary alike, is protocol 48, and so is every demo in the PRE and POST libraries. Protocol 47 is the one before it: DoD 1.3 demos from the early Steam years, 2004 and a little after. This page says what differs between the two, how DoD Studio reads and converts a protocol-47 demo, and how that was established (#239).

Measured on a community archive of 2004-2008 DoD 1.3 match demos (not in the repo: the files are other people's recordings): 103 files, 78 of protocol 47, 23 of protocol 48, and two damaged copies with no demo header. The protocol-47 set covers seven maps (dod_donner, dod_chemille, dod_anzio, dod_harrington, dod_railroad, dod_railroad2_b2, dod_lennon_b2), 71 HLTV recordings and 7 POV ones, several matches per map and many recorders.

## What the game does with one

`hw.dll` refuses a protocol-47 demo in two places, on both builds:

1. **The header.** Starting the demo checks the header's network and demo protocols against its own (48 and 5): `ERROR: demo protocol outdated` / `Demo file protocols %iN:%iD` / `Server protocol is at %iN:%iD`.
2. **`svc_serverinfo`.** Signing on checks the protocol the server info carries: `Server returned version %i, not %i`.

(Strings in both builds' `hw.dll`. Playing an unconverted demo, and a converted one, in the game is still to be checked on each build.)

## What differs between 47 and 48

Only those two numbers. Measured with `analysis/examples/protocol47_probe.rs` over all 101 readable demos:

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

## Reading one

`dem-patch` reads protocols 47 and 48 and refuses any other by number in the header parse (`dem-patch/src/protocol.rs`): "network protocol N is not supported". No other protocol has been checked against a real demo, so one is not read with rules that may not hold for it. The analyzer shows a protocol-47 demo's network protocol as 47.

## Converting one

`native::patch::protocol47` rewrites the two numbers in place: the header's protocol, and the protocol of each `svc_serverinfo` saying 47. It searches every byte from the header to the directory (or to the end of a file whose directory is missing) for a server info: its id and protocol, confirmed by a game folder, a host name and a `maps/*.bsp` where they belong. Searching the bytes rather than only the frames a walk reaches matters for a damaged demo: a broken frame length stops a walk, or carries it past a restart's server info, and a server info left at 47 makes the game refuse the copy at that restart. Over the 78 demos the search finds exactly the server infos a frame walk does. A walk of the frames by their own lengths still runs, to say where a damaged copy's frames stop fitting: the CLI and the activity log report it, since the game may not play past that point. Every other byte stays the recorder's; the length doesn't change, so the directory needs nothing. A demo with no server info to rewrite is refused, since the game would still refuse the copy.

Where a protocol-47 demo is converted:

- The Demo Analyzer shows a notice above every tab for a protocol-47 demo, with Convert: it writes `<name>_p48.dem` beside the demo, lists it in the Analyzer's file browser and opens it (`studio/src/analyzer_protocol47.js`, `studio/src-tauri/src/demo_convert.rs`). The activity log gets a line when it starts and one when it ends, with how long it took. A conversion reads and writes the file once: about half a second at most over the 78 demos above (9 to 91 MB, local disk, the CLI's start included), so the notice says "Converting…" with the button disabled rather than showing a progress bar. One of the 78 has frames that stop fitting before its end (a damaged copy: a frame zeroed 8.5 MB in, and cut short), which the activity log and the CLI report.
- `dod-studio-cli convert-47 <demo>...` writes `<name>_p48.dem` beside each demo, or `--output <file>` for one. The demo itself is only read, and the copy never goes over it: it is written to a new temporary file beside its final name (one that did not exist before, so neither the demo nor anyone else's `.part` file is ever opened for writing) and renamed into place.
- A patched copy says 48 by itself: `StreamPatcher` (Studio's patched batches, `preview_cli`) and `dod-studio-cli patch-streak` convert as they copy. The patcher logs a line when it converts one, and refuses a protocol-47 demo it finds no server info in, as the conversion does.
- Anything that plays the original file in the game (Studio's previews and Seek Between Clips batches, the in-game Load Demo window) still meets the game's refusal: convert the demo first.

The idea and the two places to change come from HLAE's DemoTools wizard, "Try to convert demo version (47 -> 48)": `AfxCppCli/old/tools/demotools/demotools.cpp` (`bSet_NetworkVersion` for the header, `bSet_ProtocolVersion` in `copy_gamedata` for the server info) and `demotoolswizard.cpp`, advancedfx commit `41b158171b275f9bb8ead737e8c7d0a159daf0af`, MIT (`third_party_notices.md`). Their tool reads each network frame's first messages one by one (`svc_nop`, `svc_time`, `svc_print`, `svc_hltv`) and stops at the first other one before the server info; ours finds the server info wherever it sits, past a damaged frame too. The measurements above are our own.

## Checking it

- Unit tests in `native/src/patch/protocol47.rs` on synthetic demos and on `test-fixtures/ci_fixture.dem` made to say 47, which converts back to the fixture byte for byte.
- `real_protocol_47_demos_convert_parse_and_analyse_the_same`, `#[ignore]`d, over a folder of real protocol-47 demos:

      DOD_PROTOCOL47_DEMOS=<folder> cargo test -p native --release --lib -- --ignored --nocapture real_protocol_47

  For each demo: only the header byte and one byte per server info change (47 to 48), the copy parses as strictly as the original, and the analyzer reads the same map, players, kills and rounds from both.
- `cargo run -p analysis --release --example protocol47_probe -- <folder>` prints the table's columns per demo.
