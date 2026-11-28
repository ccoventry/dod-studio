# Which Day of Defeat version a demo was recorded on (#1542)

R&D notes. Goal: say which DoD version (1.0, 1.1, 1.1b, 1.1c, 1.2, 1.3) a demo was recorded on, so Studio can explain old-demo problems and pick the right fix-ups. `dod::Version` already lists the versions; nothing fills it in.

## The fingerprint

Every demo's first `svc_serverinfo` carries a 16-byte MD5 of the server's `client.dll` (`SvcServerInfo::client_dll_hash`, bytes 13..29 of the message counted from its id byte). `native/examples/client_dll_hash_probe.rs` reads it from the first 64 KB of each demo and counts the values.

Survey, 2026-11-28: 533 demos (the pre-Anniversary install's `dod\` folder plus a Downloads folder of community and 2004-era demos).

| Hash | Demos | What it is |
|---|---|---|
| `00000000000000000000000000000000` | 525 | A modern 1.3 server. Says nothing about the version. |
| `2fd1fa826241265fde6417902a3d4790` | 6 | Old wire format (below). `shock_vs_3h_merderet_022304_hltv_round1/2` (Feb 23 2004, protocol 47), `chemille-gamewyze-fx-1/2`, `anzio-mts-neohunting-1/2`. |
| `2046de1cb25d49529e2a3dfe65784d56` | 2 | 1.3 wire format. `rr1-dice-xeno-1/2`. |

Every demo had a readable server info.

## What is verified

| Claim | Status |
|---|---|
| The hash is in every demo, in the same place, for protocol 47 and 48 | Verified on 533 demos. |
| A modern 1.3 server sends zeros | Verified (525 of 525 modern demos). |
| `2fd1...` is the old wire format (43 clientdata delta fields, 15 weapon_data, event fparam divisor 100, VGUIMenu size 1, WeaponList 9 bytes, `shell_small/medium/large/mg42.mdl` precached, no `models/shells.mdl`) | Verified from the demos (#1542). |
| `2046...` is already on the 1.3 wire format (39/10 fields, divisor 8192, VGUIMenu 5, WeaponList 10, `shells.mdl`) | Verified from the demos (#1542). |
| `2fd1...` is 1.1c, `2046...` is 1.2 | **Unverified.** Inferred from the release dates (Valve wiki: 1.1 Nov 14 2003, 1.1b Nov 19 2003, 1.1c Dec 10 2003, 1.2 May 19 2004) against the demo dates. No `client.dll` of 1.1 to 1.2 has been found to hash. |
| A real `client.dll` of 1.0 hashes to `dd2137a800348ffa7dd1ba0aca15338c` | Verified as a file hash, see below. Not seen in any demo. |

### The 1.0 `client.dll`

archive.org's `dod-steam_offline` item (`dod-steam_version_offline.iso`, 349 MiB) is a Steam-era offline CD holding a RAR installer, `INSTALL\DOD-SETUP.EXE` (opened with 7-Zip only, nothing run). Inside, `dod\cl_dlls\client.dll` is dated 2003-09-26, 684,494 bytes, MD5 `dd2137a800348ffa7dd1ba0aca15338c`. The same folder's `dod\liblist.gam.bef` says `version "1.0"`, and its `dod\dlls\dod.dll` (2003-09-26, MD5 `e11e8df4bd070365f56153f44b7b36ec`) precaches `shell_small`, `shell_medium`, `shell_large` and `shell_mg42` and has no `shells.mdl`: the old shell scheme, as in `2fd1...`.

Caveats. It is the Steam package's copy, not a retail CD's (the later `liblist.gam` files in the same installer were rewritten in 2006). No demo in the survey sends `dd21...`, so it is not confirmed that a server sends the plain MD5 of its `client.dll` file; it is the fingerprint a 1.0 server would send if it does.

## Secondary signals for demos whose server sent zeros

A zero hash is every modern server, so there is nothing to name. The wire format still separates the old builds from 1.3, and a demo recorded by a 1.3 client on a modern server is 1.3 by construction.

| Signal | Old (`2fd1...`) | 1.3 |
|---|---|---|
| `clientdata_t` delta fields | 43 | 39 |
| `weapon_data_t` delta fields | 15 | 10 |
| Event `fparam` divisor | 100 | 8192 |
| `VGUIMenu` registered size | 1 | 5 |
| `WeaponList` message size | 9 bytes | 10 |
| Shell models precached | `shell_small/medium/large/mg42.mdl` | `shells.mdl` |
| Network protocol | 47 (2004 HLTV demos) or 48 | 48 |

The stock 1.3 `client.dll` crashes on the first shell eject in an old-format demo unless `cl_bulletejects 0`; the hook DLL's tempent fix covers it.

## Where Studio could show it

The Demo Analyzer's Technical Specifications, one row after "Network protocol": "Recorded on: older build (fingerprint `2fd1fa82...`)" when the hash is known and non-zero, an unnamed hash printed as it is, and nothing for zeros. Naming versions waits for the missing `client.dll` files: until then the row says "older build" and "1.2 or 1.3 wire format", not a version number.

## What would settle it

- A `client.dll` (or patch installer) for 1.1, 1.1b, 1.1c and 1.2, to hash and match against `2fd1...` and `2046...`.
- Or demos whose version is known.
- A retail 1.0 CD's `client.dll`, to compare with the Steam package's.

Once `dod::server_info` (#1539) is on `dev`, the probe should call it rather than carry its own scan.
