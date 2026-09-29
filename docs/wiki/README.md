# docs/wiki

Source for the [GitHub wiki](https://github.com/ccoventry/dod-studio/wiki)
(#355). `.github/workflows/sync_wiki.yml` publishes it on every push to `main`
that touches this folder or `docs/dodstudio_commands.md`:

- each `*.md` here (except this README) becomes the wiki page of the same
  name — `Home.md` is the wiki's front page;
- the **Commands** page is generated from `docs/dodstudio_commands.md`, with
  its relative links pointed at the repo. Edit that file, not a copy here.

Build locally to check: `python .github/scripts/build_wiki.py <out-dir>`.

Add engine pages one at a time, as their own `*.md` here, and link them from
`Home.md`.
