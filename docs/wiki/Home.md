# DoD Studio

Tooling for **Day of Defeat 1.3** (GoldSrc) demo files: match analytics from
demos, and batch-recording highlight clips by driving the game with
[HLAE](https://www.advancedfx.org/).

This wiki is built from reviewed files in the
[repository](https://github.com/ccoventry/dod-studio) (`docs/wiki/`), so
edits go through a pull request like everything else. Pages edited here
directly are overwritten by the next sync.

## Pages

- **[Commands](Commands)** — every `ds_*` console variable and command
  the DoD Studio hook DLL adds to the game, with defaults and what each does.

## Before you start

DoD Studio loads HLAE and its own DLL into the game, which is what VAC looks
for. Read the README's
[Use at your own risk](https://github.com/ccoventry/dod-studio#use-at-your-own-risk)
section and
[VAC safety](https://github.com/ccoventry/dod-studio/blob/main/docs/vac_safety.md)
before you run it.

## In the repository

- [README](https://github.com/ccoventry/dod-studio#readme) — what each part of
  the project is, and how stable it is.
- [Features](https://github.com/ccoventry/dod-studio/blob/main/docs/features.md) —
  what DoD Studio does today.
- [VAC safety](https://github.com/ccoventry/dod-studio/blob/main/docs/vac_safety.md) —
  every way the game gets started, and what keeps each one away from
  VAC-secured servers.
- [Console commands](https://github.com/ccoventry/dod-studio/blob/main/docs/dodstudio_commands.md) —
  the source of the Commands page here.
- [The hook DLL](https://github.com/ccoventry/dod-studio/blob/main/goldsrc-hooks/README.md) —
  what each in-game module does, and how to build and test it.
- [Docs index](https://github.com/ccoventry/dod-studio/blob/main/docs/README.md) —
  every engineering note.
- [Releases](https://github.com/ccoventry/dod-studio/releases) — downloads.
