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
| Capture batch | `capture_engine` → `PatcherConfig::build_hlae_process` | HLAE's hook, plus ours if found | the hook DLL's connect refusal (when ours is found); HLAE's connect warning; `-insecure` |
| Launch Preview | `launch_demo_preview` → `build_hlae_process` | same | same |
| Launch Game (HLAE) | `launch_standalone_game` → `build_hlae_process` | same | same. This one opens at the main menu with no demo, so the server browser is one click away |
| `inject.exe` (manual testing, `goldsrc-hooks/README.md`) | the user, any way they like | ours only, into any running `hl.exe` | the hook DLL's connect refusal, once the DLL has hooked the engine (its log says `connect_guard: wrapped`). No HLAE, so no connect warning |

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
given. There is no HLAE in that session, so no connect warning. Until #451
that was the one route with no protection at all; the DLL's own refusal
(below) now covers it too. Its README and the tool itself still warn.

## The hook DLL refuses to join a server, except an HLTV proxy (#451)

Built from option 1 of the proposals below (decided in the 2026-09-28 review,
D2). While `dodstudio_goldsrc_hooks.dll` is loaded it wraps the engine's own
`connect`, `listen`, `retry` and `reconnect` commands, through the SDK's
command-list functions (no per-build address, both builds), and refuses to
join anything but an HLTV proxy. The console says why, and the hook log gets a
`connect_guard: refused ...` line. It is a stop, not a question, and it doesn't
depend on HLAE's hook.

**HLTV proxies can be joined** (the user's call, 2026-10-05: watching a match
through HLTV is the one online use wanted with the DLL loaded). Before a
`connect <address>` or `listen <address>` goes through, the DLL sends the
address the standard server-info query (`A2S_INFO`) off the game thread, for up
to 2 seconds. It joins only when the answer says **both** "HLTV proxy" (server
type `p`) **and** "VAC off". A game server, a proxy that reports VAC on, an
address that doesn't answer, or one with anything but plain `host:port`
characters is refused, with the reason in the console. HLAE still asks "You are
about to connect to a server" on top of that; answer Yes for the proxy.

Read from both builds of `proxy.dll`: the proxy answers the query with type `p`
and writes its VAC byte as a constant 0, so today every HLTV proxy passes and
the VAC check is there in case a proxy ever reports otherwise. A proxy that
isn't relaying a game answers nothing, so it can't be joined.

Read from both `hw.dll`s: every way the game joins a server ends in the
`connect` command. `retry` queues `connect <last server>` (or `listen`), and
so do a server's redirect and a Steam join request; the server browser's Join
button queues `connect <address>` (`ServerBrowser.dll`). So refusing `connect`
covers the server browser too, though that hasn't been tried live.

What is still allowed:

- **`connect local`**, which is how `map` joins the game's own listen server.
  No one else can be on it.
- **`retry`**, always: it only queues `connect` or `listen` for the last
  address, and that is checked like any other.
- **`reconnect` after `connect local` or an HLTV proxy.** A `changelevel` on
  your own map sends `reconnect`.
- **`reconnect` while a demo plays.** There it joins nothing, so it is left as
  the engine has it.

Limits:

- A `+connect` on the game's launch line runs before the DLL has wrapped
  anything. DoD Studio never puts one there.
- If the DLL can't find `connect` in the engine's command list, nothing is
  refused, and it says so in the console and the log.
- `GOLDSRC_HOOKS_ALLOW_CONNECT=1`, set before the game starts, turns it off,
  for someone who knowingly tests on their own server. The log says so.

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

## Proposed: a hard stop

These change behaviour, so they were for review before anyone built them.
Option 1 is built (see above); 2 to 4 are not.

1. **The hook DLL refuses `connect`.** Built in #451.
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
