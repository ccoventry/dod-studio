# Staying VAC-safe

DoD Studio changes the game while it runs. HLAE's `AfxHookGoldSrc.dll` and DoD
Studio's own `dodstudio_goldsrc_hooks.dll` are loaded into `hl.exe` and patch
`hw.dll` and `client.dll` in memory (detours and byte patches). Nothing is
changed on disk, but modified game code in a running `hl.exe` is what VAC looks
for. **Joining a VAC-secured server from a game DoD Studio started, or from any
game with either DLL loaded, is a ban risk.**

This page is the audit behind issue #373: every way the game gets started or
injected, what protects each one, and what is still open. It doesn't try to
say whether any particular action would or wouldn't get an account banned. The
rules below are deliberately cautious.

## The rules

1. **Keep a separate copy of Half-Life just for movies.** Copy the whole
   `Half-Life` folder from `steamapps\common` to a new folder (for example
   `Half-Life - Movies`) and point DoD Studio at that copy's `hl.exe`. Play
   online only with Steam's own install, and never point DoD Studio at it.
2. **Only play demos in the movie copy.** Never join a server with it, not even
   a friend's or an insecure one. If HLAE's "You are about to connect to a
   server" warning appears, answer **No**.
3. **Never inject the hook DLL by hand into a game you play online with.**
   `inject.exe` (below) skips every safeguard DoD Studio has.

## Every way the game is started

| Route | What starts `hl.exe` | What loads into it | Protection today |
| --- | --- | --- | --- |
| Capture batch | `capture_engine` → `PatcherConfig::build_hlae_process` | HLAE's hook, plus ours if found | HLAE's connect warning; `-insecure` |
| Launch Preview | `launch_demo_preview` → `build_hlae_process` | same | same |
| Launch Game (HLAE) | `launch_standalone_game` → `build_hlae_process` | same | same. This one opens at the main menu with no demo, so the server browser is one click away |
| `inject.exe` (manual testing, `goldsrc-hooks/README.md`) | the user, any way they like | ours only, into any running `hl.exe` | none: no HLAE, so no connect warning |

Every DoD Studio launch goes through the one function,
`native/src/patch/types.rs`'s `build_hlae_process`. It always starts the game
through HLAE's `-customLoader` with `AfxHookGoldSrc.dll` as the first hook DLL,
and adds `-insecure` to the game's command line. Nothing else in the app starts
or injects into `hl.exe`. (The other `Command::new` calls start FFmpeg, OBS,
Explorer and `taskkill`.)

### HLAE's connect warning

`AfxHookGoldSrc.dll` hooks the engine's `connect` command. Before a connection
goes through, it shows:

> WARNING: You are about to connect to a server. It is strongly recommended to
> NOT connect to any server while HLAE is running! ... Do you want to continue
> connecting?

Answering No aborts the connection and closes the game. (Strings read from
`AfxHookGoldSrc.dll`, HLAE Latest, 2026-09-24.) The server browser, `retry` and
a `+connect` on the launch line are expected to run `connect` too, and so get
the warning, but that hasn't been tested.

Two limits:

- **It is a question, not a stop.** Yes connects.
- **It can fail quietly.** The DLL also contains
  `HLAE warning: Failed hooking connect`. If that hook doesn't install (a
  different engine build, say), there is no warning at all, and nothing in
  DoD Studio would notice.

### `-insecure`

The pre-Anniversary `hw.dll` checks for `-insecure` and has the messages
`VAC secure mode disabled.` and `You are running software that is not
compatible with Secure servers.` What exactly it blocks when the game tries to
join a secure server as a client hasn't been tested, so treat it as a second
line of defence, not a guarantee.

### The hook DLL loaded without HLAE

`inject.exe` loads `dodstudio_goldsrc_hooks.dll` into whatever process ID it is
given. There is no HLAE in that session, so no connect warning, and the DLL
itself does not check where it is. That is the one route with no protection at
all. Its README and the tool itself now say so.

## Warnings added (#373)

- **README.md** points here, under "Staying VAC-safe".
- **Configuration → Paths:** a warning under *Half-Life Executable* when the
  chosen `hl.exe` is in Steam's own `steamapps\common\Half-Life` folder, which
  is normally the copy people play online with.
- **`goldsrc-hooks/README.md`, manual testing:** never inject into a game you
  play online with; this route skips HLAE's warning.
- **`inject.exe`** prints the same warning every time it runs.
- **`goldsrc-hooks/tools/hd/README.md`:** the HD images are plain files, but
  the hook that loads them is the ban risk, so build them into the movie copy.

## Proposed, not built: a hard stop

These change behaviour, so they are for review before anyone builds them.

1. **The hook DLL refuses `connect`.** `goldsrc-hooks` already registers
   console commands. It could wrap the engine's own `connect` handler the way
   HLAE does and refuse outright, with a console message, rather than asking.
   This covers `inject.exe` sessions and a failed HLAE hook. The cost: a
   deliberate connect to a local test server would need an opt-out, for
   example a `GOLDSRC_HOOKS_ALLOW_CONNECT=1` environment variable.
2. **The hook DLL only activates when DoD Studio started the game.**
   `build_hlae_process` would set an environment variable (say
   `DODSTUDIO_LAUNCHED=1`) and the DLL would install nothing without it. This
   stops the DLL from ever acting in a session DoD Studio didn't start. Manual
   testing with `inject.exe` would then need that variable set before starting
   `hl.exe`, the same way its other variables already are.
3. **Refuse to launch Steam's own install.** Turn the Configuration warning
   into a block that needs a one-time "I understand" to get past. Stronger,
   but it would also stop someone who knowingly keeps only one install for
   demo work and never plays online.
4. **A one-time notice on first launch** explaining the separate-copy rule,
   stored in settings once acknowledged.

Option 1 is the one that closes the only unprotected route (`inject.exe`) and
the silent-failure case, without getting in the way of normal use.
